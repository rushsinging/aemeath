pub const DEFAULT_MAX_TOOL_RESPONSE_BYTES: usize = 1_048_576;
pub fn limit_tool_response(output: &str, max_bytes: usize) -> String {
    if output.len() <= max_bytes {
        return output.to_string();
    }

    let truncate_at = output.floor_char_boundary(max_bytes);

    let notice = format!(
        "[Output truncated: original {} bytes, limit {} bytes]",
        output.len(),
        max_bytes
    );

    if truncate_at == 0 {
        notice
    } else {
        format!("{}\n\n{}", &output[..truncate_at], notice) // allow unsafe_text_op: floor_char_boundary derived truncate_at
    }
}
