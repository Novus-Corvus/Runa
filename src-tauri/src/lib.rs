// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
use serde::Serialize;
use std::fs;
use std::path::Path;

#[derive(Serialize)]
struct FileNode {
    name: String,
    path: String,
    is_dir: bool,
    children: Option<Vec<FileNode>>,
}

fn build_tree(path: &Path) -> FileNode {
    let metadata: fs::Metadata = fs::metadata(path).unwrap();

    if metadata.is_dir() {
        let children: Vec<FileNode> = fs::read_dir(path)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                let path = entry.path();

                // only allow certain file types
                if let Some(ext) = path.extension() {
                    ext == "md" || ext == "txt"
                } else {
                    true // allow folders
                }
            })
            .map(|entry| build_tree(&entry.path()))
            .collect();

        FileNode {
            name: path.file_name().unwrap().to_string_lossy().to_string(),
            path: path.to_string_lossy().to_string(),
            is_dir: true,
            children: Some(children),
        }
    } else {
        FileNode {
            name: path.file_name().unwrap().to_string_lossy().to_string(),
            path: path.to_string_lossy().to_string(),
            is_dir: false,
            children: None,
        }
    }
}

#[tauri::command]
fn get_workspace_tree(path: String) -> FileNode {
    build_tree(Path::new(&path))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![get_workspace_tree])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
