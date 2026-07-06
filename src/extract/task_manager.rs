use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::Utc;
use serde::Serialize;
use uuid::Uuid;





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

// ─── Task Progress (legacy, section-based) ────────────────────────

/// Progress information for a running task (section-based pipeline).
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

// ─── Step-based Progress (for full-doc extraction) ────────────────

/// A single step within an extraction task (shown in the frontend).
#[derive(Debug, Clone, Serialize)]
pub struct ExtractionStep {
    /// Human-readable label shown in the UI.
    pub label: String,
    /// One of: "pending", "running", "completed", "failed".
    pub status: String,
    /// Progress within this step (0.0–100.0).
    pub progress_pct: f64,
    /// Optional detail text (e.g. "15/20 vertices created").
    pub detail: Option<String>,
}

/// Create a default set of extraction steps (all pending).
pub fn default_extraction_steps() -> Vec<ExtractionStep> {
    vec![
        ExtractionStep {
            label: "Reading document content".to_string(),
            status: "pending".to_string(),
            progress_pct: 0.0,
            detail: None,
        },
        ExtractionStep {
            label: "Calling LLM to extract knowledge".to_string(),
            status: "pending".to_string(),
            progress_pct: 0.0,
            detail: None,
        },
        ExtractionStep {
            label: "Creating graph vertices".to_string(),
            status: "pending".to_string(),
            progress_pct: 0.0,
            detail: None,
        },
        ExtractionStep {
            label: "Creating graph edges".to_string(),
            status: "pending".to_string(),
            progress_pct: 0.0,
            detail: None,
        },
    ]
}

/// Update a step in the steps vec by label.
pub fn update_step(steps: &mut Vec<ExtractionStep>, label: &str, status: &str, progress_pct: f64, detail: Option<&str>) {
    if let Some(step) = steps.iter_mut().find(|s| s.label == label) {
        step.status = status.to_string();
        step.progress_pct = progress_pct;
        step.detail = detail.map(|d| d.to_string());
    }
}

/// Compute overall progress from step progress values (equal weight per step).
pub fn compute_overall_pct(steps: &[ExtractionStep]) -> f64 {
    if steps.is_empty() {
        return 0.0;
    }
    let all_done = steps.iter().all(|s| s.status == "completed");
    if all_done {
        return 100.0;
    }
    let total: f64 = steps.iter().map(|s| {
        match s.status.as_str() {
            "completed" => 100.0,
            "running" => s.progress_pct,
            _ => 0.0,
        }
    }).sum();
    total / steps.len() as f64
}

// ─── Extraction Task ─────────────────────────────────────────────

/// A single extraction task with its lifecycle.
#[derive(Debug, Clone, Serialize)]
pub struct ExtractionTask {
    pub task_id: String,
    pub status: TaskStatus,
    /// Legacy section-based progress (used by old pipeline).
    pub progress: Option<TaskProgress>,
    /// New step-based progress (used by full-doc extraction).
    pub steps: Vec<ExtractionStep>,
    /// Overall progress percentage (0.0–100.0), derived from steps.
    pub overall_pct: f64,
    pub stats: Option<ExtractionStats>,
    pub error: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub graph_name: String,
    /// Source name / description embedded in the task
    pub source_name: String,
    /// Document ID if this task was created for a document extraction
    pub document_id: Option<String>,
}

// ─── Task Manager ────────────────────────────────────────────────

/// Manages async extraction tasks.
#[derive(Clone)]
pub struct ExtractionTaskManager {
    pub tasks: Arc<Mutex<HashMap<String, ExtractionTask>>>,
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
            steps: Vec::new(),
            overall_pct: 0.0,
            stats: None,
            error: None,
            created_at: Utc::now().to_rfc3339(),
            started_at: None,
            completed_at: None,
            graph_name: graph_name.to_string(),
            source_name: source_name.to_string(),
            document_id: None,
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

    /// Update a task's steps (step-based progress).
    pub fn update_task_steps(&self, task_id: &str, steps: Vec<ExtractionStep>) {
        let mut tasks = self.tasks.lock().unwrap();
        if let Some(task) = tasks.get_mut(task_id) {
            task.steps = steps.clone();
            task.overall_pct = compute_overall_pct(&steps);
        }
    }

    /// Mark a specific step as completed and update overall progress.
    pub fn complete_step(&self, task_id: &str, step_label: &str) {
        let mut tasks = self.tasks.lock().unwrap();
        if let Some(task) = tasks.get_mut(task_id) {
            update_step(&mut task.steps, step_label, "completed", 100.0, None);
            task.overall_pct = compute_overall_pct(&task.steps);
        }
    }

    /// Set task status + error + mark steps as failed.
    pub fn fail_task(&self, task_id: &str, error: String) {
        let mut tasks = self.tasks.lock().unwrap();
        if let Some(task) = tasks.get_mut(task_id) {
            task.status = TaskStatus::Failed;
            task.error = Some(error);
            task.completed_at = Some(Utc::now().to_rfc3339());
            for step in &mut task.steps {
                if step.status == "running" {
                    step.status = "failed".to_string();
                }
            }
            task.overall_pct = compute_overall_pct(&task.steps);
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

}

impl Default for ExtractionTaskManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics from a document extraction run.
#[derive(Debug, Default, Clone, Serialize)]
pub struct ExtractionStats {
    pub total_sections: usize,
    pub processed_sections: usize,
    pub total_entities: usize,
    pub total_relations: usize,
    pub new_vertices: usize,
    pub new_edges: usize,
}

// ─── Helper to get step-based task response for frontend ──────────

/// Simplified task view returned to the frontend for document extraction tasks.
/// Picks the best representation (steps for new, progress for legacy).
#[derive(Debug, Clone, Serialize)]
pub struct TaskResponse {
    pub task_id: String,
    pub document_id: Option<String>,
    pub status: String,
    pub steps: Vec<ExtractionStep>,
    pub overall_pct: f64,
    pub stats: Option<ExtractionStats>,
    pub error: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

impl From<ExtractionTask> for TaskResponse {
    fn from(task: ExtractionTask) -> Self {
        TaskResponse {
            task_id: task.task_id,
            document_id: task.document_id,
            status: match task.status {
                TaskStatus::Pending => "pending".to_string(),
                TaskStatus::Running => "running".to_string(),
                TaskStatus::Completed => "completed".to_string(),
                TaskStatus::Failed => "failed".to_string(),
            },
            steps: if task.steps.is_empty() {
                // Legacy task: synthesize a simple step from section progress
                if let Some(ref p) = task.progress {
                    vec![ExtractionStep {
                        label: "Processing sections".to_string(),
                        status: match task.status {
                            TaskStatus::Completed => "completed".to_string(),
                            TaskStatus::Failed => "failed".to_string(),
                            _ => "running".to_string(),
                        },
                        progress_pct: p.percentage(),
                        detail: Some(format!("{}/{} sections", p.processed_sections, p.total_sections)),
                    }]
                } else {
                    vec![ExtractionStep {
                        label: match task.status {
                            TaskStatus::Pending => "Waiting...".to_string(),
                            TaskStatus::Failed => "Failed".to_string(),
                            _ => "Processing".to_string(),
                        }.to_string(),
                        status: match task.status {
                            TaskStatus::Pending => "pending".to_string(),
                            TaskStatus::Failed => "failed".to_string(),
                            _ => "running".to_string(),
                        },
                        progress_pct: 0.0,
                        detail: None,
                    }]
                }
            } else {
                task.steps
            },
            overall_pct: task.overall_pct,
            stats: task.stats,
            error: task.error,
            created_at: task.created_at,
            started_at: task.started_at,
            completed_at: task.completed_at,
        }
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
        assert_eq!(tasks[0].task_id, id2);
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

    #[test]
    fn test_default_steps_created() {
        let steps = default_extraction_steps();
        assert_eq!(steps.len(), 4);
        assert_eq!(steps[0].status, "pending");
    }

    #[test]
    fn test_update_step() {
        let mut steps = default_extraction_steps();
        update_step(&mut steps, "Calling LLM to extract knowledge", "running", 50.0, Some("50% done"));
        assert_eq!(steps[1].status, "running");
        assert!((steps[1].progress_pct - 50.0).abs() < 0.001);
    }

    #[test]
    fn test_compute_overall_pct() {
        let mut steps = default_extraction_steps();
        // First step completed
        update_step(&mut steps, "Reading document content", "completed", 100.0, None);
        let pct = compute_overall_pct(&steps);
        assert!((pct - 25.0).abs() < 0.001); // 1/4 done = 25%
    }

    #[test]
    fn test_task_response_from_legacy_task() {
        let mgr = ExtractionTaskManager::new();
        let id = mgr.create_task("default", "test.md");
        mgr.update_task(&id, TaskStatus::Running, Some(TaskProgress {
            processed_sections: 3,
            total_sections: 10,
            current_heading: "Working".to_string(),
        }), None, None);
        let task = mgr.get_task(&id).unwrap();
        let resp: TaskResponse = task.into();
        assert_eq!(resp.status, "running");
        assert_eq!(resp.steps.len(), 1);
        assert_eq!(resp.steps[0].label, "Processing sections");
    }
}