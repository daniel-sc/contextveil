//! Line-based terminal interaction for setup.
//!
//! `CLI-002` requires an interactive TTY, and `CLI-003` requires human-readable
//! output. The TTY check happens at the command boundary; this type only moves
//! lines, so setup can be driven by a scripted transcript in tests.
//!
//! Every untrusted string must be rendered through `crate::sanitize` before it
//! reaches these writers (`SEC-006`).

use std::io::{BufRead, BufReader, Read, Write};

/// The user ended the phase without confirming it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cancelled;

/// A line-based prompt surface.
pub struct Terminal<'a> {
    input: Box<dyn BufRead + 'a>,
    output: Box<dyn Write + 'a>,
}

impl<'a> Terminal<'a> {
    pub fn new(input: impl Read + 'a, output: impl Write + 'a) -> Self {
        Self {
            input: Box::new(BufReader::new(input)),
            output: Box::new(output),
        }
    }

    pub fn line(&mut self, text: &str) {
        let _ = writeln!(self.output, "{text}");
    }

    pub fn blank(&mut self) {
        let _ = writeln!(self.output);
    }

    /// Asks a free-form question. End of input cancels the phase.
    pub fn ask(&mut self, question: &str) -> Result<String, Cancelled> {
        let _ = write!(self.output, "{question} ");
        let _ = self.output.flush();
        let mut answer = String::new();
        match self.input.read_line(&mut answer) {
            Ok(0) | Err(_) => Err(Cancelled),
            Ok(_) => Ok(answer.trim_end_matches(['\n', '\r']).to_string()),
        }
    }

    /// Asks a yes or no question with an explicit default.
    pub fn confirm(&mut self, question: &str, default: bool) -> Result<bool, Cancelled> {
        let hint = if default { "[Y/n]" } else { "[y/N]" };
        let answer = self.ask(&format!("{question} {hint}"))?;
        Ok(match answer.trim().to_ascii_lowercase().as_str() {
            "" => default,
            "y" | "yes" => true,
            _ => false,
        })
    }

    /// Flushes buffered output. Called before the process exits.
    pub fn finish(&mut self) {
        let _ = self.output.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scripted(script: &str) -> (Terminal<'_>, std::rc::Rc<std::cell::RefCell<Vec<u8>>>) {
        let output = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let terminal = Terminal::new(
            std::io::Cursor::new(script.to_string()),
            Sink(output.clone()),
        );
        (terminal, output)
    }

    struct Sink(std::rc::Rc<std::cell::RefCell<Vec<u8>>>);

    impl Write for Sink {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            self.0.borrow_mut().extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn answers_are_trimmed_of_line_endings_only() {
        let (mut terminal, _) = scripted("  spaced  \r\nsecond\n");
        assert_eq!(terminal.ask("q?").as_deref(), Ok("  spaced  "));
        assert_eq!(terminal.ask("q?").as_deref(), Ok("second"));
    }

    #[test]
    fn end_of_input_cancels() {
        let (mut terminal, _) = scripted("");
        assert_eq!(terminal.ask("q?"), Err(Cancelled));
        assert_eq!(terminal.confirm("ok?", true), Err(Cancelled));
    }

    #[test]
    fn confirmation_defaults_apply_to_an_empty_answer() {
        let (mut terminal, _) = scripted("\n\ny\nyes\nn\nanything\n");
        assert_eq!(terminal.confirm("a", true), Ok(true));
        assert_eq!(terminal.confirm("b", false), Ok(false));
        assert_eq!(terminal.confirm("c", false), Ok(true));
        assert_eq!(terminal.confirm("d", false), Ok(true));
        assert_eq!(terminal.confirm("e", true), Ok(false));
        assert_eq!(terminal.confirm("f", true), Ok(false));
    }

    #[test]
    fn output_is_written_verbatim() {
        let (mut terminal, output) = scripted("\n");
        terminal.line("hello");
        let _ = terminal.confirm("ok?", true);
        let written = String::from_utf8(output.borrow().clone()).expect("UTF-8 output");
        assert_eq!(written, "hello\nok? [Y/n] ");
    }
}
