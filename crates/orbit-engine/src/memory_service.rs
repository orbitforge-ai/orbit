use crate::executor::memory::MemoryClient;

/// mem0 cloud API key — embedded at build time via MEM0_API_KEY env var.
/// Mirrors how SUPABASE_URL / SUPABASE_ANON_KEY are handled in auth.rs.
const MEM0_API_KEY: Option<&str> = option_env!("MEM0_API_KEY");

/// Managed state holding the memory client.
/// Created once at startup; None if MEM0_API_KEY was not set at build time.
#[derive(Clone)]
pub struct MemoryServiceState {
    pub client: MemoryClient,
}

impl MemoryServiceState {
    /// Construct the state if a mem0 API key was embedded at build time.
    /// Returns None (not an error) if the key is absent — app works without memory.
    pub fn try_create() -> Option<Self> {
        let key = MEM0_API_KEY?.trim();
        if key.is_empty() {
            return None;
        }
        Some(Self {
            client: MemoryClient::new(key.to_string()),
        })
    }
}
