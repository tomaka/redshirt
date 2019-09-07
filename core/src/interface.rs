
pub struct Interface {
    name: String,
    functions: Vec<Function>,
}

struct Function {
    name: String,
    signature: wasmi::Signature,
}

impl Interface {
    pub fn hash(&self) -> Vec<u8> {
        // TODO: proper implementation
        self.name.as_bytes().to_vec()
    }
}
