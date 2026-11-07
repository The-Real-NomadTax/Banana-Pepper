use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InputFrame {
    pub sequence: u64,
    pub focused: bool,
    pub sources: Vec<u16>,
    pub held: Vec<u16>,
    pub edges: Vec<(u16, bool)>,
}
impl InputFrame {
    pub fn valid(&self) -> bool {
        self.sources.len() <= 256 && self.held.len() <= 256 && self.edges.len() <= 1024
    }
}
