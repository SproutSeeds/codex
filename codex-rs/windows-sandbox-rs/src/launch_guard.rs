#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) const LEGACY_SANDBOX_SETUP_REQUIRED_ERROR: &str =
    "Windows restricted-token sandbox cannot launch reliably from this non-interactive or network-logon session until elevated Windows sandbox setup has completed. Run Codex once from an interactive administrator session to finish elevated setup, or configure windows.sandbox = \"elevated\".";

#[cfg_attr(not(any(target_os = "windows", test)), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProcessTokenContext {
    pub(crate) session_id: u32,
    pub(crate) has_interactive_sid: bool,
    pub(crate) has_remote_interactive_sid: bool,
    pub(crate) has_network_sid: bool,
}

impl ProcessTokenContext {
    #[cfg_attr(not(any(target_os = "windows", test)), allow(dead_code))]
    pub(crate) fn needs_elevated_runner_fallback(self) -> bool {
        let has_interactive_logon = self.has_interactive_sid || self.has_remote_interactive_sid;
        self.has_network_sid || (self.session_id == 0 && !has_interactive_logon)
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn current_process_token_context() -> anyhow::Result<ProcessTokenContext> {
    use crate::token::get_current_token_for_restriction;
    use crate::token::token_group_sid_strings;
    use crate::token::token_session_id;
    use windows_sys::Win32::Foundation::CloseHandle;

    const SID_INTERACTIVE: &str = "S-1-5-4";
    const SID_NETWORK: &str = "S-1-5-2";
    const SID_REMOTE_INTERACTIVE: &str = "S-1-5-14";

    let token = unsafe { get_current_token_for_restriction()? };
    let result = unsafe {
        let group_sids = token_group_sid_strings(token)?;
        Ok(ProcessTokenContext {
            session_id: token_session_id(token)?,
            has_interactive_sid: group_sids.iter().any(|sid| sid == SID_INTERACTIVE),
            has_remote_interactive_sid: group_sids.iter().any(|sid| sid == SID_REMOTE_INTERACTIVE),
            has_network_sid: group_sids.iter().any(|sid| sid == SID_NETWORK),
        })
    };
    unsafe {
        CloseHandle(token);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_logon_tokens_require_elevated_fallback() {
        let context = ProcessTokenContext {
            session_id: 0,
            has_interactive_sid: false,
            has_remote_interactive_sid: false,
            has_network_sid: true,
        };

        assert!(context.needs_elevated_runner_fallback());
    }

    #[test]
    fn session_zero_without_interactive_groups_requires_elevated_fallback() {
        let context = ProcessTokenContext {
            session_id: 0,
            has_interactive_sid: false,
            has_remote_interactive_sid: false,
            has_network_sid: false,
        };

        assert!(context.needs_elevated_runner_fallback());
    }

    #[test]
    fn local_interactive_tokens_stay_on_legacy_runner() {
        let context = ProcessTokenContext {
            session_id: 1,
            has_interactive_sid: true,
            has_remote_interactive_sid: false,
            has_network_sid: false,
        };

        assert!(!context.needs_elevated_runner_fallback());
    }

    #[test]
    fn remote_interactive_tokens_stay_on_legacy_runner() {
        let context = ProcessTokenContext {
            session_id: 2,
            has_interactive_sid: false,
            has_remote_interactive_sid: true,
            has_network_sid: false,
        };

        assert!(!context.needs_elevated_runner_fallback());
    }
}
