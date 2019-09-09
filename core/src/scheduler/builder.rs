// Copyright(c) 2019 Pierre Krieger

pub struct CoreBuilder<TNet> {
    network_access: Option<TNet>,
}

impl CoreBuilder<()> {
    pub fn new() -> Self {
        CoreBuilder {
            network_access: None,
        }
    }
}

impl<TNet> CoreBuilder<TNet> {
    pub fn with_core_module() {

    }

    pub fn build(self) {
        
    }
}
