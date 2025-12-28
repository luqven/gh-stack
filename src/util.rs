use dialoguer::Input;

pub fn loop_until_confirm(prompt: &str) {
    let prompt = format!("{} Type 'yes' to continue", prompt);
    loop {
        let result: String = Input::new()
            .with_prompt(&prompt)
            .interact_text()
            .unwrap();
        match &result[..] {
            "yes" => return,
            _ => continue,
        }
    }
}
