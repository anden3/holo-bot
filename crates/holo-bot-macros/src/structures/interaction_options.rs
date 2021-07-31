use super::{Checks, Permissions};

#[derive(Debug, Default)]
pub struct InteractionOptions {
    pub checks: Checks,
    pub allowed_roles: Vec<String>,
    pub required_permissions: Permissions,
    pub owners_only: bool,
    pub owner_privilege: bool,
}

impl InteractionOptions {
    #[inline]
    pub fn new() -> Self {
        Default::default()
    }
}
