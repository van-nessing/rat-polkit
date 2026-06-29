use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub enum UserResponse<'a> {
    Submit {
        cookie: &'a str,
        username: &'a str,
        password: &'a str,
    },
    Cancel {
        cookie: &'a str,
    },
}

#[derive(Serialize, Deserialize)]
pub enum AuthResponses {
    Success,
    Cancel,
    Retry { message: String },
}

#[derive(Serialize, Deserialize)]
pub struct StartAuth<'a> {
    pub prompt: &'a str,
    pub message: &'a str,
    pub users: Vec<String>,
}
