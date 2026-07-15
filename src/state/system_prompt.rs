use std::{fs::read_to_string, path::Path, sync::LazyLock};

#[derive(Debug, Clone)]
pub struct SystemPrompt(pub String);
impl SystemPrompt {
    pub fn new() -> Self {
        // create dir if not exists
        if !Path::new(".appleby").exists() {
            std::fs::create_dir_all(".appleby").unwrap();
        }
        // create file if not exists 
        if !Path::new(".appleby/system_prompt.txt").exists() {
            std::fs::write(".appleby/system_prompt.txt", SystemPrompt::default().0).unwrap();
        }
        let system_prompt = read_to_string(".appleby/system_prompt.txt").unwrap();
        Self(system_prompt)
    }
}

impl Default for SystemPrompt {
    fn default() -> Self {
        Self("
You are a helpful assistant. You can use the following tools to help the user.
If the tool fails to meet the requirements, you can invoke the function ask for help
and clearly and explicitly state what kind of tool you need to fulfill the requirements.
        ".to_string())
    }
}

pub static SYSTEM_PROMPT: LazyLock<SystemPrompt> = LazyLock::new(|| SystemPrompt::new());
