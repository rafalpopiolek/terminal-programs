/// ANSI colors used by the renderer. When disabled every method returns the text unchanged.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    enabled: bool,
}

impl Palette {
    pub fn new(enabled: bool) -> Self {
        Self { enabled }
    }

    pub fn dir(self, text: &str) -> String {
        self.paint(text, "1;34")
    }

    pub fn executable(self, text: &str) -> String {
        self.paint(text, "1;32")
    }

    pub fn symlink(self, text: &str) -> String {
        self.paint(text, "36")
    }

    pub fn broken(self, text: &str) -> String {
        self.paint(text, "31")
    }

    /// Secondary information: tree guides, hints, dotfiles.
    pub fn dim(self, text: &str) -> String {
        self.paint(text, "90")
    }

    fn paint(self, text: &str, sgr: &str) -> String {
        if self.enabled && !text.is_empty() {
            format!("\x1b[{sgr}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }
}
