// @TODO:
//  - make goose a cargo library
//  - create goosefile by creating a cargo-app with a goose dependency
//  - implementing a load test should then be more or less writing this goosefile
//  - loadtests are shipped as a single compiled binary

use crate::goose::{GooseTaskSets, GooseTaskSet, GooseClient, GooseTask};

impl GooseTaskSets {
    // @TODO: auto-write this function with metaprogramming helpers
    pub fn initialize_goosefile(&mut self) {
        trace!("initialize_goosefile");

        // Register a website task set and contained tasks
        let mut website_tasks = GooseTaskSet::new("WebsiteTasks").set_weight(10);
        website_tasks.register_task(GooseTask::new("/index.html").set_weight(6).set_function(GooseClient::website_task_index));
        website_tasks.register_task(GooseTask::new("/story.html").set_weight(9).set_function(GooseClient::website_task_story));
        website_tasks.register_task(GooseTask::new("/about.html").set_weight(3).set_function(GooseClient::website_task_about));
        self.register_taskset(website_tasks);
    }
}

impl GooseClient {
    fn website_task_index(&mut self) {
        let _response = self.get("http://apache.fosciana/");
    }

    fn website_task_story(&mut self) {
        let _response = self.get("http://apache.fosciana/story.html");
    }

    fn website_task_about(&mut self) {
        let _response = self.get("http://apache.fosciana/about.html");
    }
}

/*
class WebsiteUser(HttpLocust):
    task_set = WebsiteTasks
    wait_time = between(5, 15)
*/