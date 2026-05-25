use ::runtime::api::image::{is_image_file, process_image_file};
use crate::render::TerminalRenderer;
use ::runtime::api::core::message::Message;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use std::path::Path;

use super::commands::{handle_slash_command, SlashResult};
use super::image_input::extract_image_paths;
use super::PendingImages;

pub(super) enum InputAction {
    Continue,
    Exit,
    Ready,
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn read_and_prepare_input(
    rl: &mut DefaultEditor,
    messages: &mut Vec<Message>,
    system_prompt_text: &str,
    context_size: usize,
    total_input_tokens: u64,
    total_output_tokens: u64,
    total_api_calls: u64,
    session_id: &str,
    cwd: &Path,
    pending_images: &PendingImages,
    resumed_session: Option<&::runtime::api::core::session::Session>,
    allow_all: &mut bool,
    skills: &std::collections::HashMap<String, ::runtime::api::core::skill::Skill>,
) -> InputAction {
    {
        let images = pending_images.lock().unwrap();
        if !images.is_empty() {
            TerminalRenderer::print_pending_images(images.len());
        }
    }

    TerminalRenderer::print_user_prompt();
    let input = match rl.readline("") {
        Ok(line) => line.trim().to_string(),
        Err(ReadlineError::Interrupted) => {
            println!("(use /exit to quit)");
            return InputAction::Continue;
        }
        Err(ReadlineError::Eof) => return InputAction::Exit,
        Err(e) => {
            eprintln!("input error: {e}");
            return InputAction::Exit;
        }
    };

    if input.is_empty() {
        return InputAction::Continue;
    }

    if input.starts_with('/') {
        match handle_slash_command(
            &input,
            messages,
            system_prompt_text,
            context_size,
            total_input_tokens,
            total_output_tokens,
            total_api_calls,
            session_id,
            cwd,
            pending_images,
            resumed_session,
            allow_all,
            skills,
        )
        .await
        {
            SlashResult::Continue => return InputAction::Continue,
            SlashResult::Exit => return InputAction::Exit,
            SlashResult::NotFound => {
                eprintln!("unknown command: {input}. Type /help for available commands.");
                return InputAction::Continue;
            }
            SlashResult::InjectMessage(prompt) => {
                messages.push(Message::user(&prompt));
                let _ = rl.add_history_entry(&input);
                return InputAction::Ready;
            }
        }
    }

    if is_image_file(&input) {
        let full_path = if Path::new(&input).is_absolute() {
            input.clone()
        } else {
            cwd.join(&input).to_string_lossy().to_string()
        };
        match process_image_file(&full_path).await {
            Ok(img) => {
                let size = img.original_size;
                pending_images.lock().unwrap().push(img);
                println!("[image added ({} bytes)]", size);
                println!("  Type your message and press Enter to send with the image.");
                return InputAction::Continue;
            }
            Err(e) => {
                eprintln!("error: {e}");
                return InputAction::Continue;
            }
        }
    }

    let (clean_input, inline_images) = extract_image_paths(&input, cwd).await;
    let _ = rl.add_history_entry(&input);

    {
        let mut pending = pending_images.lock().unwrap();
        pending.extend(inline_images);
    }

    let images = pending_images.lock().unwrap().drain(..).collect::<Vec<_>>();
    let msg_text = if clean_input.is_empty() {
        &input
    } else {
        &clean_input
    };

    if images.is_empty() {
        messages.push(Message::user(msg_text));
    } else {
        let image_data: Vec<(String, String)> = images
            .iter()
            .map(|img| (img.base64.clone(), img.media_type.clone()))
            .collect();
        messages.push(Message::user_with_images(msg_text, image_data));
        for (i, img) in images.iter().enumerate() {
            println!("[sent image {}: {} bytes]", i + 1, img.final_size);
        }
    }

    InputAction::Ready
}
