use std::{collections::{BTreeSet, HashMap}, process::Child, rc::Rc, sync::RwLock};

pub struct BgJobList {
    jobs: HashMap<usize, Rc<RwLock<BackgroundJob>>>,
    jobs_ages: Vec<usize>,
    current_job: usize,
    recyclable: BTreeSet<usize>,
}

impl BgJobList {
    pub fn new() -> Self {
        Self { jobs: HashMap::new(), current_job: 0, jobs_ages: Vec::new(), recyclable: BTreeSet::new() }
    }
    pub fn insert_job(&mut self, job: BackgroundJob) -> usize {
        let internal_id;
        if let Some(recycled) = self.recyclable.pop_first() {
            internal_id = recycled;
        } else {
            self.current_job += 1;
            internal_id = self.current_job;
        }
        self.jobs_ages.push(internal_id);
        self.jobs.insert(internal_id, Rc::new(RwLock::new(job)));
        internal_id
    }
    pub fn get_recent(&self) -> (usize, usize) { // (latest, second)
        let jobs_len = self.jobs_ages.len();
        if jobs_len >= 2 {
            (self.jobs_ages[jobs_len-1], self.jobs_ages[jobs_len-2])
        } else if jobs_len == 1 {
            (self.jobs_ages[0], 0)
        } else {
            (0, 0)
        }
    }
    pub fn reap(&mut self, print_reaped: bool) {
        let mut to_remove = vec![];
        for (i_id, job) in &self.jobs {
            if job.write().unwrap().get_status() == "Done" {
                to_remove.push(*i_id);
            }
        }
        let (latest, second) = self.get_recent();
        for internal_id in &to_remove {
            let old = self.jobs.remove(&internal_id);
            if print_reaped && let Some(old) = old {
                let age = match *internal_id {
                    n if n == latest => '+',
                    n if n == second => '-',
                    _ => ' ',
                };
                println!("[{}]{}  Done                    {}", internal_id, age, old.read().unwrap().get_command());
            }
            let age_position = self.jobs_ages.iter().position(|i_id| i_id == internal_id).unwrap();
            self.jobs_ages.remove(age_position);
            self.recyclable.insert(*internal_id);
        }
    }
}

impl<'a> IntoIterator for &'a BgJobList {
    type Item = (&'a usize, &'a Rc<RwLock<BackgroundJob>>);
    type IntoIter = std::collections::hash_map::Iter<'a, usize, Rc<RwLock<BackgroundJob>>>;

    fn into_iter(self) -> Self::IntoIter {
        self.jobs.iter()
    }
}

pub struct BackgroundJob {
    command: String,
    job: Child,
}

impl BackgroundJob {
    pub fn new(command: &str, job: Child) -> Self {
        Self { command: command.to_string(), job }
    }
    pub fn get_command(&self) -> String {
        self.command.clone()
    }
    pub fn get_status(&mut self) -> String {
        match self.job.try_wait() {
            Ok(Some(_)) => "Done".to_string(),
            Ok(None) => "Running".to_string(),
            Err(_) => "".to_string(),
        }
    }
}