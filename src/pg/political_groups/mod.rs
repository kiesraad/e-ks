//! Political group setup and maintenance flows.
mod actions;
mod extractors;
mod forms;
mod pages;
mod paths;
mod steps;

pub use forms::PoliticalGroupForm;
pub use pages::router;
pub use paths::PoliticalGroupUpdatePath;
pub use steps::{PoliticalGroupStep, PoliticalGroupSteps};
