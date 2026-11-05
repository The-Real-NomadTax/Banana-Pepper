//! Internal switch model, inferred from this build's g11/st1/cn1/tst table.
#[derive(Debug, Default)]
pub struct Switches {
    pub counters: [u32; 20],
    pub held: u32,
    pub dirty: bool,
}
impl Switches {
    pub fn set(&mut self, index: usize, down: bool) {
        if index == 0 || index >= self.counters.len() {
            return;
        }
        let mask = 1 << index;
        if (self.held & mask != 0) == down {
            return;
        }
        self.held ^= mask;
        self.counters[index] = self.counters[index].wrapping_add(1);
        self.dirty = true;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn switches_count_transitions_not_key_repeat() {
        let mut state = Switches::default();
        state.set(11, true);
        state.set(11, true);
        assert_eq!(state.counters[11], 1);
        state.set(11, false);
        state.set(11, false);
        assert_eq!(state.counters[11], 2);
        assert!(state.dirty);
    }
}
