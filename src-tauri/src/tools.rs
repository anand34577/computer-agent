//! Tool definitions for the agent and for chat, plus JSON action mode for
//! models that cannot call tools natively.

use crate::llm::{ToolCall, ToolDef};
use serde_json::{json, Value};

fn def(name: &str, description: &str, parameters: Value) -> ToolDef {
    ToolDef { name: name.into(), description: description.into(), parameters }
}

fn target_props() -> Value {
    json!({
        "element": {"type":"integer","description":"Number of a tagged element (preferred)"},
        "x": {"type":"integer","description":"X pixel in the screenshot (only if no element number fits)"},
        "y": {"type":"integer","description":"Y pixel in the screenshot"}
    })
}

pub fn agent_tools() -> Vec<ToolDef> {
    let mut click = target_props();
    click["button"] = json!({"type":"string","enum":["left","right","middle"]});
    click["double"] = json!({"type":"boolean"});
    let mut typ = target_props();
    typ["text"] = json!({"type":"string"});
    typ["submit"] = json!({"type":"boolean","description":"Press Enter after typing"});
    let mut scroll = target_props();
    scroll["direction"] = json!({"type":"string","enum":["up","down","left","right"]});
    scroll["amount"] = json!({"type":"integer","description":"Wheel notches, default 5"});
    vec![
        def("click", "Click an element (by number) or a screenshot position.", json!({"type":"object","properties": click})),
        def("type_text", "Type text, optionally into an element first (focuses it). Never use for passwords.", json!({"type":"object","properties": typ,"required":["text"]})),
        def("press_keys", "Press a key or shortcut, e.g. 'enter', 'ctrl+s', 'alt+tab', 'win'.", json!({"type":"object","properties":{"keys":{"type":"string"}},"required":["keys"]})),
        def("scroll", "Scroll the view (over an element or position if given).", json!({"type":"object","properties": scroll,"required":["direction"]})),
        def("hover", "Move the pointer over an element or position.", json!({"type":"object","properties": target_props()})),
        def("drag", "Drag from one element/position to another.", json!({"type":"object","properties":{
            "from_element":{"type":"integer"},"from_x":{"type":"integer"},"from_y":{"type":"integer"},
            "to_element":{"type":"integer"},"to_x":{"type":"integer"},"to_y":{"type":"integer"}}})),
        def("open_app", "Launch an application by name (e.g. 'notepad', 'excel', 'chrome').", json!({"type":"object","properties":{"name":{"type":"string"}},"required":["name"]})),
        def("open_url", "Open a web address in the default browser.", json!({"type":"object","properties":{"url":{"type":"string"}},"required":["url"]})),
        def("switch_window", "Bring an open window to the front by (part of) its title.", json!({"type":"object","properties":{"title":{"type":"string"}},"required":["title"]})),
        def("wait", "Wait for loading, in seconds (max 10).", json!({"type":"object","properties":{"seconds":{"type":"number"}},"required":["seconds"]})),
        def("list_files", "List a folder. Accepts full paths or 'downloads', 'documents', 'desktop', 'pictures'.", json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]})),
        def("read_file", "Read a text file.", json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]})),
        def("move_file", "Move a file or folder (undoable). 'to' may be a folder.", json!({"type":"object","properties":{"from":{"type":"string"},"to":{"type":"string"}},"required":["from","to"]})),
        def("rename_file", "Rename a file (undoable). 'to' can be just the new name.", json!({"type":"object","properties":{"from":{"type":"string"},"to":{"type":"string"}},"required":["from","to"]})),
        def("create_folder", "Create a folder (undoable).", json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]})),
        def("delete_file", "Delete a file or folder into the undo holding area (restorable).", json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]})),
        def("write_file", "Create or overwrite a text file (previous version kept for undo).", json!({"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"]})),
        def("clipboard_read", "Read the clipboard text.", json!({"type":"object","properties":{}})),
        def("clipboard_write", "Put text on the clipboard.", json!({"type":"object","properties":{"text":{"type":"string"}},"required":["text"]})),
        def("ask_user", "Ask the user a question when the goal is ambiguous or you need information.", json!({"type":"object","properties":{"question":{"type":"string"}},"required":["question"]})),
        def("done", "Finish the task with a short summary for the user.", json!({"type":"object","properties":{"summary":{"type":"string"},"success":{"type":"boolean"}},"required":["summary","success"]})),
    ]
}

pub fn chat_tools(perms: &std::collections::HashMap<String, String>) -> Vec<ToolDef> {
    let all = vec![
        def("list_files", "List the files in a folder on the user's computer.", json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]})),
        def("read_file", "Read a text file on the user's computer.", json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]})),
        def("fetch_url", "Fetch a web page and return its readable text.", json!({"type":"object","properties":{"url":{"type":"string"}},"required":["url"]})),
        def("clipboard_read", "Read the clipboard text.", json!({"type":"object","properties":{}})),
        def("clipboard_write", "Put text on the clipboard.", json!({"type":"object","properties":{"text":{"type":"string"}},"required":["text"]})),
        def("current_time", "Get the current local date and time.", json!({"type":"object","properties":{}})),
    ];
    all.into_iter().filter(|t| perms.get(&t.name).map(|p| p != "off").unwrap_or(true)).collect()
}

/// Instructions for models without native tool calling.
pub fn json_mode_instructions(tools: &[ToolDef]) -> String {
    let mut s = String::from(
        "\n\nYou cannot call tools natively. Reply with ONLY one JSON object and nothing else, in this form:\n{\"thought\": \"what you see and why\", \"tool\": \"<tool name>\", \"args\": { ... }}\nAvailable tools:\n",
    );
    for t in tools {
        let props = t.parameters["properties"].as_object().map(|o| o.keys().cloned().collect::<Vec<_>>().join(", ")).unwrap_or_default();
        s.push_str(&format!("- {}({}): {}\n", t.name, props, t.description));
    }
    s
}

/// Extracts a JSON action from free text.
pub fn parse_json_action(text: &str, n: usize) -> Option<(String, ToolCall)> {
    let v = extract_json(text)?;
    let name = v["tool"].as_str().or(v["name"].as_str()).or(v["action"].as_str())?.to_string();
    let args = if v["args"].is_object() { v["args"].clone() } else if v["arguments"].is_object() { v["arguments"].clone() } else { json!({}) };
    let thought = v["thought"].as_str().unwrap_or("").to_string();
    Some((thought, ToolCall { id: format!("json_{n}"), name, arguments: args }))
}

/// Finds the first balanced JSON object in free text.
pub fn extract_json(text: &str) -> Option<Value> {
    let start = text.find('{')?;
    let bytes = text.as_bytes();
    let mut depth = 0i32;
    let mut in_str = false;
    let mut esc = false;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_str {
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return serde_json::from_str(&text[start..=i]).ok();
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_json_actions_from_chatty_text() {
        let t = "Sure! Here is my action:\n```json\n{\"thought\": \"click {save}\", \"tool\": \"click\", \"args\": {\"element\": 14}}\n```";
        let (thought, tc) = parse_json_action(t, 3).unwrap();
        assert_eq!(thought, "click {save}");
        assert_eq!(tc.name, "click");
        assert_eq!(tc.arguments["element"], 14);
        assert!(parse_json_action("no json here", 0).is_none());
    }

    #[test]
    fn tool_schemas_are_objects() {
        for t in agent_tools() {
            assert_eq!(t.parameters["type"], "object", "{}", t.name);
        }
    }
}
