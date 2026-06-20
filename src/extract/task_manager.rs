use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::Utc;
use serde::Serialize;
use uuid::Uuid;

use super::config::ExtractionConfig;
use super::pipeline::ExtractionStats;
use super::extract_content_raw_with_nn_and_progress;

use crate::graph::Graph;
use crate::neuron::NeuralNetwork;

// ─── Task Status ─────────────────────────────────────────────────

/// Status of an extraction task.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

// ─── Task Progress ───────────────────────────────────────────────

/// Progress information for a running task.
#[derive(Debug, Clone, Serialize)]
pub struct TaskProgress {
    pub processed_sections: usize,
    pub total_sections: usize,
    pub current_heading: String,
}

impl TaskProgress {
    pub fn percentage(&self) -> f64 {
        if self.total_sections == 0 {
            return 0.0;
        }
        (self.processed_sections as f64 / self.total_sections as f64) * 100.0
    }
}

// ─── Extraction Task ─────────────────────────────────────────────

/// A single extraction task with its lifecycle.
#[derive(Debug, Clone, Serialize)]
pub struct ExtractionTask {
    pub task_id: String,
    pub status: TaskStatus,
    pub progress: Option<TaskProgress>,
    pub stats: Option<ExtractionStats>,
    pub error: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub graph_name: String,
    /// Source name / description embedded in the task
    pub source_name: String,
}

// ─── Task Manager ────────────────────────────────────────────────

/// Manages async extraction tasks.
#[derive(Clone)]
pub struct ExtractionTaskManager {
    tasks: Arc<Mutex<HashMap<String, ExtractionTask>>>,
}

impl ExtractionTaskManager {
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Create a new task and return its ID.
    pub fn create_task(&self, graph_name: &str, source_name: &str) -> String {
        let task_id = Uuid::new_v4().to_string();
        let task = ExtractionTask {
            task_id: task_id.clone(),
            status: TaskStatus::Pending,
            progress: None,
            stats: None,
            error: None,
            created_at: Utc::now().to_rfc3339(),
            started_at: None,
            completed_at: None,
            graph_name: graph_name.to_string(),
            source_name: source_name.to_string(),
        };
        self.tasks.lock().unwrap().insert(task_id.clone(), task);
        task_id
    }

    /// Update a task's status and progress.
    pub fn update_task(
        &self,
        task_id: &str,
        status: TaskStatus,
        progress: Option<TaskProgress>,
        stats: Option<ExtractionStats>,
        error: Option<String>,
    ) {
        let mut tasks = self.tasks.lock().unwrap();
        if let Some(task) = tasks.get_mut(task_id) {
            let status_clone = status.clone();
            task.status = status;
            if status_clone == TaskStatus::Running {
                task.started_at = Some(Utc::now().to_rfc3339());
            }
            if let Some(p) = progress {
                task.progress = Some(p);
            }
            if let Some(s) = stats {
                task.stats = Some(s);
            }
            if let Some(e) = error {
                task.error = Some(e);
            }
            if status_clone == TaskStatus::Completed || status_clone == TaskStatus::Failed {
                task.completed_at = Some(Utc::now().to_rfc3339());
            }
        }
    }

    /// Get a task by ID.
    pub fn get_task(&self, task_id: &str) -> Option<ExtractionTask> {
        self.tasks.lock().unwrap().get(task_id).cloned()
    }

    /// List all tasks, newest first.
    pub fn list_tasks(&self) -> Vec<ExtractionTask> {
        let mut tasks: Vec<ExtractionTask> = self
            .tasks
            .lock()
            .unwrap()
            .values()
            .cloned()
            .collect();
        tasks.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        tasks
    }

    /// Submit an extraction to run in the background.
    ///
    /// Returns the task_id immediately. The extraction runs via `tokio::spawn`,
    /// updating progress as sections are processed.
    pub fn submit_extraction(
        &self,
        config: ExtractionConfig,
        content: String,
        source_name: String,
        graph: Arc<Mutex<Graph>>,
        neural: Arc<Mutex<NeuralNetwork>>,
        graph_name: String,
    ) -> String {
        let task_id = self.create_task(&graph_name, &source_name);
        let task_id_clone = task_id.clone();
        let manager = self.clone();

        tokio::spawn(async move {
            manager.update_task(&task_id_clone, TaskStatus::Running, None, None, None);

            // Parse sections first to get total count for progress
            let sections = match crate::extract::document::split_sections(&content) {
                Ok(s) => s,
                Err(e) => {
                    manager.update_task(
                        &task_id_clone,
                        TaskStatus::Failed,
                        None,
                        None,
                        Some(format!("Failed to parse document: {}", e)),
                    );
                    return;
                }
            };
            let total_sections = sections.len();

            manager.update_task(
                &task_id_clone,
                TaskStatus::Running,
                Some(TaskProgress {
                    processed_sections: 0,
                    total_sections,
                    current_heading: "Parsing document...".to_string(),
                }),
                None,
                None,
            );

            // Run extraction with progress callback for live updates
            let mgr = manager.clone();
            let tid = task_id_clone.clone();
            let cb: super::pipeline::ProgressCallback = Some(Arc::new(move |processed, total, heading| {
                mgr.update_task(
                    &tid,
                    TaskStatus::Running,
                    Some(TaskProgress {
                        processed_sections: processed,
                        total_sections: total,
                        current_heading: heading.to_string(),
                    }),
                    None,
                    None,
                );
            }));

            let result = extract_content_raw_with_nn_and_progress(
                &config,
                &content,
                &source_name,
                &graph,
                &neural,
                cb,
            )
            .await;

            match result {
                Ok(stats) => {
                    manager.update_task(
                        &task_id_clone,
                        TaskStatus::Completed,
                        Some(TaskProgress {
                            processed_sections: stats.processed_sections,
                            total_sections: stats.total_sections,
                            current_heading: "Done".to_string(),
                        }),
                        Some(stats),
                        None,
                    );
                }
                Err(e) => {
                    manager.update_task(
                        &task_id_clone,
                        TaskStatus::Failed,
                        Some(TaskProgress {
                            processed_sections: 0,
                            total_sections,
                            current_heading: "Error".to_string(),
                        }),
                        None,
                        Some(e),
                    );
                }
            }
        });

        task_id
    }
}

impl Default for ExtractionTaskManager {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_and_get_task() {
        let mgr = ExtractionTaskManager::new();
        let id = mgr.create_task("default", "test.md");
        let task = mgr.get_task(&id).unwrap();
        assert_eq!(task.status, TaskStatus::Pending);
        assert_eq!(task.graph_name, "default");
        assert_eq!(task.source_name, "test.md");
    }

    #[test]
    fn test_update_task_status() {
        let mgr = ExtractionTaskManager::new();
        let id = mgr.create_task("default", "test.md");
        mgr.update_task(&id, TaskStatus::Running, Some(TaskProgress {
            processed_sections: 1,
            total_sections: 5,
            current_heading: "Intro".to_string(),
        }), None, None);
        let task = mgr.get_task(&id).unwrap();
        assert_eq!(task.status, TaskStatus::Running);
        assert!(task.started_at.is_some());
        assert_eq!(task.progress.unwrap().processed_sections, 1);
    }

    #[test]
    fn test_list_tasks_ordered() {
        let mgr = ExtractionTaskManager::new();
        let id1 = mgr.create_task("default", "a.md");
        std::thread::sleep(std::time::Duration::from_millis(10));
        let id2 = mgr.create_task("default", "b.md");
        let tasks = mgr.list_tasks();
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].task_id, id2); // newest first
        assert_eq!(tasks[1].task_id, id1);
    }

    #[test]
    fn test_progress_percentage() {
        let p = TaskProgress {
            processed_sections: 3,
            total_sections: 10,
            current_heading: "Test".to_string(),
        };
        assert!((p.percentage() - 30.0).abs() < 0.001);
        let p2 = TaskProgress {
            processed_sections: 0,
            total_sections: 0,
            current_heading: "Empty".to_string(),
        };
        assert!((p2.percentage() - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_get_nonexistent_task() {
        let mgr = ExtractionTaskManager::new();
        assert!(mgr.get_task("nonexistent").is_none());
    }
}
