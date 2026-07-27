#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

mod commands;

use std::sync::Mutex;

use engine_project::document::DocumentHandle;
use engine_tiles::TileCache;
use commands::AppState;

fn main() {
    // Initialize app state
    use engine_project::types::DocumentId;
    
    let document = engine_project::Document::new(DocumentId::new(1), 800, 600);
    let doc_handle = DocumentHandle::new(document);
    let tile_cache = TileCache::new(256 * 1024 * 1024); // 256 MB budget
    
    let app_state = AppState {
        document_handle: doc_handle,
        tile_cache,
        image_data: Mutex::new(None),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            // Document commands
            commands::new_document,
            commands::get_document_snapshot,
            
            // Layer commands
            commands::add_layer,
            commands::remove_layer,
            commands::set_layer_props,
            commands::reorder_layer,
            
            // Filter commands
            commands::add_filter,
            commands::update_filter,
            commands::remove_filter,
            
            // Image commands
            commands::load_image,
            commands::render_preview,
            commands::export_image,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    #[test]
    fn stub_compiles() {
        assert!(true);
    }
}
