use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use serde::{Deserialize, Serialize};

#[allow(dead_code)]
const DOCUMENTS_DIR: &str = "data/documents";

/// Document metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: String,
    pub title: String,
    pub tags: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
    pub graph_name: String,
}

/// Document index stored on disk.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct DocumentIndex {
    documents: Vec<Document>,
}

/// Manages document storage.
#[derive(Clone)]
pub struct DocumentManager {
    index_path: PathBuf,
    docs_dir: PathBuf,
    index: Arc<Mutex<DocumentIndex>>,
}

impl DocumentManager {
    pub fn new(data_dir: &str) -> Self {
        let docs_dir = PathBuf::from(data_dir).join("documents");
        let index_path = docs_dir.join("index.json");
        fs::create_dir_all(&docs_dir).ok();

        let index = if index_path.exists() {
            fs::read_to_string(&index_path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default()
        } else {
            DocumentIndex::default()
        };

        Self {
            index_path,
            docs_dir,
            index: Arc::new(Mutex::new(index)),
        }
    }

    fn save_index(&self) {
        if let Ok(index) = self.index.lock() {
            let json = serde_json::to_string_pretty(&*index).unwrap();
            fs::write(&self.index_path, json).ok();
        }
    }

    /// List all documents.
    pub fn list(&self) -> Vec<Document> {
        self.index.lock().unwrap().documents.clone()
    }

    /// Get a single document by ID.
    pub fn get(&self, id: &str) -> Option<Document> {
        self.index.lock().unwrap().documents.iter().find(|d| d.id == id).cloned()
    }

    /// Get document content. Searches YYMMDD subdirectories for the file.
    pub fn get_content(&self, id: &str) -> Option<String> {
        // Scan date subdirectories for the file
        if let Ok(entries) = fs::read_dir(&self.docs_dir) {
            for entry in entries.flatten() {
                let date_dir = entry.path();
                if date_dir.is_dir() {
                    let file_path = date_dir.join(format!("{}.md", id));
                    if file_path.exists() {
                        if let Ok(content) = fs::read_to_string(&file_path) {
                            return Some(content);
                        }
                    }
                }
            }
        }
        // Fallback: flat directory (for backward compat)
        let path = self.docs_dir.join(format!("{}.md", id));
        fs::read_to_string(&path).ok()
    }

    fn date_dir(&self) -> PathBuf {
        let now = chrono::Utc::now();
        let yymmdd = now.format("%y%m%d").to_string();
        self.docs_dir.join(&yymmdd)
    }

    /// Add a new document. Stores content to file and metadata to index.
    pub fn add(&self, id: &str, title: &str, content: &str, tags: &[String], graph_name: &str) -> Document {
        // Save content under YYMMDD subdirectory
        let date_dir = self.date_dir();
        fs::create_dir_all(&date_dir).ok();
        let file_path = date_dir.join(format!("{}.md", id));
        fs::write(&file_path, content).ok();

        let now = chrono::Utc::now().to_rfc3339();
        let doc = Document {
            id: id.to_string(),
            title: title.to_string(),
            tags: tags.to_vec(),
            created_at: now.clone(),
            updated_at: now,
            graph_name: graph_name.to_string(),
        };

        let mut index = self.index.lock().unwrap();
        index.documents.push(doc.clone());
        drop(index);
        self.save_index();
        doc
    }

    /// Update document content and metadata.
    pub fn update(&self, id: &str, title: &str, tags: &[String], graph_name: Option<&str>) -> Option<Document> {
        let now = chrono::Utc::now().to_rfc3339();
        let mut index = self.index.lock().unwrap();
        if let Some(doc) = index.documents.iter_mut().find(|d| d.id == id) {
            doc.title = title.to_string();
            doc.tags = tags.to_vec();
            if let Some(g) = graph_name {
                doc.graph_name = g.to_string();
            }
            doc.updated_at = now;
            let result = doc.clone();
            drop(index);
            self.save_index();
            return Some(result);
        }
        None
    }

    /// Delete a document and its content file.
    pub fn delete(&self, id: &str) -> bool {
        // Try to remove from date subdirectories
        if let Ok(entries) = fs::read_dir(&self.docs_dir) {
            for entry in entries.flatten() {
                let date_dir = entry.path();
                if date_dir.is_dir() {
                    let file_path = date_dir.join(format!("{}.md", id));
                    fs::remove_file(&file_path).ok();
                }
            }
        }
        // Also remove flat file (backward compat)
        let file_path = self.docs_dir.join(format!("{}.md", id));
        fs::remove_file(&file_path).ok();

        let mut index = self.index.lock().unwrap();
        let len = index.documents.len();
        index.documents.retain(|d| d.id != id);
        let removed = index.documents.len() < len;
        drop(index);
        if removed {
            self.save_index();
        }
        removed
    }
}
