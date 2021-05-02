#[derive(Default, Debug)]
pub(crate) struct SessionInstance {
    content: Vec<String>,
    turn: u32,
    our_offset: u8,
}

impl SessionInstance {
    pub(crate) fn new(offset: u8) -> SessionInstance {
        SessionInstance {
            content: Vec::new(),
            turn: 0,
            our_offset: offset,
        }
    }

    pub(crate) fn can_submit(&self) -> bool {
        self.turn % self.our_offset as u32 == 0
    }

    pub(crate) fn submit(&mut self, new_part: String) {
        self.content.push(new_part);
        self.turn += 1;
    }
}
