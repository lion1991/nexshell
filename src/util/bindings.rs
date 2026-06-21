use std::borrow::Cow;
use std::collections::HashMap;

use enum_iterator::{all, Sequence};
use lazy_static::lazy_static;
use warpui::keymap::{CustomTag, Keystroke};
use warpui::platform::OperatingSystem;

// CustomActions are attached to menu items, and may be attached to Bindings.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Sequence)]
#[repr(isize)]
pub enum CustomAction {
    NewTab,
    NewFile,
    ShowAboutWarp,
    ShowSettings,
    ConfigureKeybindings,
    ShowAccount,
    ShowAppearance,
    ReferAFriend,
    ViewChangelog,
    FocusInput,
    ClearBlocks,
    AddNextOccurrence,
    AddCursorAbove,
    AddCursorBelow,
    CycleNextSession,
    CyclePrevSession,
    Cut,
    Copy,
    Paste,
    Undo,
    Redo,
    CommandPalette,
    AISearch,
    ClearEditor,
    Find,
    SelectAll,
    Workflows,
    HistorySearch,
    SaveCurrentConfig,
    History,
    IncreaseFontSize,
    DecreaseFontSize,
    ResetFontSize,
    IncreaseZoom,
    DecreaseZoom,
    ResetZoom,
    RenameTab,
    SplitPaneRight,
    SplitPaneLeft,
    SplitPaneUp,
    SplitPaneDown,
    MoveTabLeft,
    MoveTabRight,
    ActivateNextTab,
    ActivatePreviousTab,
    ActivateNextPane,
    ActivatePreviousPane,
    NavigationPalette,
    SelectBlockAbove,
    SelectBlockBelow,
    SelectAllBlocks,
    CreateBlockPermalink,
    ToggleBookmarkBlock,
    FindWithinBlock,
    CopyBlock,
    CopyBlockCommand,
    CopyBlockOutput,
    ViewSharedBlocks,
    CloseTab,
    CloseOtherTabs,
    CloseTabsRight,
    ToggleMaximizePane,
    LaunchConfigPalette,
    FilesPalette,
    TriggerWelcomeBlock,
    CommandSearch,
    ToggleResourceCenter,
    ToggleKeybindingsPage,
    ScrollToTopOfSelectedBlocks,
    ScrollToBottomOfSelectedBlocks,
    ToggleSyncAllTerminalInputsInAllTabs,
    ToggleSyncTerminalInputsInCurrentTab,
    DisableSyncTerminalInputs,
    ReopenClosedSession,
    ToggleWarpDrive,
    AddWindow,
    CloseCurrentSession,
    CloseWindow,
    NewPersonalWorkflow,
    NewPersonalNotebook,
    NewPersonalEnvVars,
    NewTeamWorkflow,
    NewTeamNotebook,
    NewTeamEnvVars,
    SearchDrive,
    OpenTeamSettings,
    ShareCurrentSession,
    SharePaneContents,
    #[cfg(windows)]
    WindowsPaste,
    #[cfg(windows)]
    WindowsCopy,
    /// Also applies to legacy Warp AI (toggles the panel)
    NewAgentModePane,
    /// Also applies to legacy Warp AI (attaches the selection to the panel editor)
    AttachSelectionAsAgentModeContext,
    OpenAIFactCollection,
    OpenMCPServerCollection,
    ToggleProjectExplorer,
    NewPersonalAIPrompt,
    NewTeamAIPrompt,
    OpenRepository,
    NewTerminalTab,
    NewAgentTab,
    GoToLine,
    ToggleGlobalSearch,
    ToggleConversationListView,
}

lazy_static! {
    /// Maps for converting from custom tags back to the action enum
    /// This layer of indirection is necessary because the UI framework can't
    /// know about particular Warp specific actions, so it deals with all actions
    /// as plain isizes.  Within Warp though we want to deal with them as the enum type.
    pub static ref CUSTOM_TAG_TO_ACTION: HashMap<isize, CustomAction> = HashMap::from_iter(all::<CustomAction>().map(|action| {
        (action as isize, action)
    }));

}

impl From<CustomAction> for CustomTag {
    fn from(action: CustomAction) -> Self {
        action as CustomTag
    }
}

impl From<CustomTag> for CustomAction {
    fn from(tag: CustomTag) -> Self {
        *CUSTOM_TAG_TO_ACTION
            .get(&tag)
            .expect("All custom actions are handled.")
    }
}

pub fn custom_tag_to_keystroke(custom: CustomTag) -> Option<Keystroke> {
    match custom.into() {
        CustomAction::FocusInput => Keystroke::parse(cmd_or_ctrl_shift("l")).ok(),
        CustomAction::NewTab => Keystroke::parse(cmd_or_ctrl_shift("t")).ok(),
        CustomAction::Cut => Keystroke::parse("cmdorctrl-x").ok(),
        CustomAction::Copy => Keystroke::parse(cmd_or_ctrl_shift("c")).ok(),
        CustomAction::Paste => Keystroke::parse(cmd_or_ctrl_shift("v")).ok(),
        #[cfg(windows)]
        CustomAction::WindowsPaste => Keystroke::parse("ctrl-v").ok(),
        #[cfg(windows)]
        CustomAction::WindowsCopy => Keystroke::parse("ctrl-c").ok(),
        CustomAction::Undo => Keystroke::parse("cmdorctrl-z").ok(),
        CustomAction::Redo => Keystroke::parse("cmdorctrl-shift-Z").ok(),
        CustomAction::ClearEditor => Keystroke::parse("ctrl-c").ok(),
        CustomAction::CycleNextSession => Keystroke::parse("ctrl-tab").ok(),
        CustomAction::CyclePrevSession => Keystroke::parse("ctrl-shift-tab").ok(),
        CustomAction::ShowSettings => Keystroke::parse("cmdorctrl-,").ok(),
        CustomAction::AddNextOccurrence => Keystroke::parse("ctrl-g").ok(),
        CustomAction::AddCursorAbove => Keystroke::parse("ctrl-shift-up").ok(),
        CustomAction::AddCursorBelow => Keystroke::parse("ctrl-shift-down").ok(),
        CustomAction::CommandPalette => Keystroke::parse(cmd_or_ctrl_shift("p")).ok(),
        CustomAction::AISearch => Keystroke::parse("ctrl-`").ok(),
        CustomAction::Find => Keystroke::parse(cmd_or_ctrl_shift("f")).ok(),
        CustomAction::SelectAll => Keystroke::parse("cmdorctrl-a").ok(),
        CustomAction::CommandSearch => Keystroke::parse("ctrl-r").ok(),
        CustomAction::Workflows => Keystroke::parse("ctrl-shift-R").ok(),
        CustomAction::History => Keystroke::parse("up").ok(),
        CustomAction::IncreaseFontSize => Keystroke::parse("shift-cmdorctrl-+").ok(),
        CustomAction::DecreaseFontSize => Keystroke::parse("shift-cmdorctrl-_").ok(),
        CustomAction::ResetFontSize => Keystroke::parse("cmdorctrl-0").ok(),
        CustomAction::IncreaseZoom => Keystroke::parse("cmdorctrl-=").ok(),
        CustomAction::DecreaseZoom => Keystroke::parse("cmdorctrl--").ok(),
        CustomAction::ResetZoom => Keystroke::parse("cmdorctrl-0").ok(),
        CustomAction::SplitPaneRight => Keystroke::parse(cmd_or_ctrl_shift("d")).ok(),
        CustomAction::SplitPaneDown => {
            if OperatingSystem::get().is_mac() {
                Keystroke::parse("cmd-shift-D").ok()
            } else {
                // On non-Mac platforms, we can't use `ctrl-shift-D` for `SplitPaneRight` since
                // we already  use that for `SplitPaneRight` above. Instead we use
                // `ctrl-shift-E`, which matches what Hyper uses. See https://github.com/vercel/hyper/blob/9c72409f5138c03a5a74fcc4dba9109217b4524a/app/keymaps/linux.json#L32.
                Keystroke::parse("ctrl-shift-E").ok()
            }
        }
        CustomAction::MoveTabLeft => {
            if OperatingSystem::get().is_mac() {
                Keystroke::parse("shift-ctrl-left").ok()
            } else {
                Keystroke::parse("shift-ctrl-pageup").ok()
            }
        }
        CustomAction::MoveTabRight => {
            if OperatingSystem::get().is_mac() {
                Keystroke::parse("shift-ctrl-right").ok()
            } else {
                Keystroke::parse("shift-ctrl-pagedown").ok()
            }
        }
        CustomAction::ActivateNextTab => {
            if OperatingSystem::get().is_mac() {
                Keystroke::parse("shift-cmd-}").ok()
            } else {
                Keystroke::parse("ctrl-pagedown").ok()
            }
        }
        CustomAction::ActivatePreviousTab => {
            if OperatingSystem::get().is_mac() {
                Keystroke::parse("shift-cmd-{").ok()
            } else {
                Keystroke::parse("ctrl-pageup").ok()
            }
        }
        CustomAction::ActivateNextPane => {
            if OperatingSystem::get().is_mac() {
                Keystroke::parse("cmd-]").ok()
            } else {
                Keystroke::parse("ctrl-shift-}").ok()
            }
        }
        CustomAction::ActivatePreviousPane => {
            if OperatingSystem::get().is_mac() {
                Keystroke::parse("cmd-[").ok()
            } else {
                Keystroke::parse("ctrl-shift-{").ok()
            }
        }
        CustomAction::NavigationPalette => mac_only_keystroke("cmd-shift-P"),
        CustomAction::LaunchConfigPalette => mac_only_keystroke("ctrl-cmd-l"),
        CustomAction::FilesPalette => Keystroke::parse(cmd_or_ctrl_shift("o")).ok(),
        CustomAction::ClearBlocks => Keystroke::parse(cmd_or_ctrl_shift("k")).ok(),
        CustomAction::SelectBlockAbove => Keystroke::parse("cmdorctrl-up").ok(),
        CustomAction::SelectBlockBelow => Keystroke::parse("cmdorctrl-down").ok(),
        // Set this to mac-only. On Linux this conflicts with the binding to save a workflow.
        CustomAction::CreateBlockPermalink => mac_only_keystroke("cmd-shift-S"),
        CustomAction::ToggleBookmarkBlock => Keystroke::parse(cmd_or_ctrl_shift("b")).ok(),
        CustomAction::CopyBlockOutput => Keystroke::parse("cmdorctrl-alt-shift-C").ok(),
        // Set this to mac-only. On Linux this conflicts with the general binding to copy.
        CustomAction::CopyBlockCommand => mac_only_keystroke("cmd-shift-C"),
        // Set this to mac-only. On Linux this conflicts with the cmd-enter keybindings
        // (used for actions on the input suggestions menu, and for accepting passive code diffs).
        CustomAction::ToggleMaximizePane => mac_only_keystroke("cmd-shift-enter"),
        // Note: The base character '/' is used instead of '?' as mac registers keybindings
        // differently compared to the app which saves the resulting character used with shift
        // TODO: resolve these keybinding differences
        CustomAction::ToggleResourceCenter => Keystroke::parse("ctrl-shift-/").ok(),
        CustomAction::ToggleKeybindingsPage => Keystroke::parse("cmdorctrl-/").ok(),
        CustomAction::ScrollToTopOfSelectedBlocks => Keystroke::parse("cmdorctrl-shift-up").ok(),
        CustomAction::ScrollToBottomOfSelectedBlocks => {
            Keystroke::parse("cmdorctrl-shift-down").ok()
        }
        CustomAction::CopyBlock => Keystroke::parse(cmd_or_ctrl_shift("c")).ok(),
        CustomAction::FindWithinBlock => Keystroke::parse(cmd_or_ctrl_shift("f")).ok(),
        CustomAction::ToggleSyncTerminalInputsInCurrentTab => {
            Keystroke::parse("alt-cmdorctrl-i").ok()
        }
        CustomAction::ReopenClosedSession => {
            if OperatingSystem::get().is_mac() {
                Keystroke::parse("cmd-shift-T").ok()
            } else {
                // Use a custom keybinding for linux/windows since the binding would otherwise
                // conflict with the binding for creating a new tab.
                Keystroke::parse("ctrl-alt-t").ok()
            }
        }

        // This is one of the app's hardcoded keybindings.
        CustomAction::AddWindow => Keystroke::parse(cmd_or_ctrl_shift("n")).ok(),
        CustomAction::ToggleWarpDrive => {
            if OperatingSystem::get().is_mac() {
                Keystroke::parse("cmd-\\").ok()
            } else {
                Keystroke::parse("ctrl-shift-|").ok()
            }
        }
        CustomAction::CloseWindow => mac_only_keystroke("cmd-shift-W"),
        CustomAction::CloseCurrentSession => Keystroke::parse(cmd_or_ctrl_shift("w")).ok(),
        CustomAction::ViewChangelog => Keystroke::parse(cmd_or_ctrl_shift("alt-o")).ok(),
        CustomAction::NewAgentModePane => Keystroke::parse("ctrl-space").ok(),
        CustomAction::AttachSelectionAsAgentModeContext => {
            Keystroke::parse("ctrl-shift-space").ok()
        }
        CustomAction::ToggleProjectExplorer => {
            if OperatingSystem::get().is_mac() {
                Keystroke::parse("ctrl-1").ok()
            } else {
                Keystroke::parse("alt-1").ok()
            }
        }
        CustomAction::OpenRepository => {
            if OperatingSystem::get().is_mac() {
                Keystroke::parse("cmd-shift-O").ok()
            } else {
                Keystroke::parse("alt-shift-O").ok()
            }
        }
        CustomAction::GoToLine => Keystroke::parse("ctrl-g").ok(),
        CustomAction::ToggleGlobalSearch => {
            if OperatingSystem::get().is_mac() {
                Keystroke::parse("ctrl-3").ok()
            } else {
                Keystroke::parse("alt-3").ok()
            }
        }
        CustomAction::ToggleConversationListView => {
            if OperatingSystem::get().is_mac() {
                Keystroke::parse("ctrl-2").ok()
            } else {
                Keystroke::parse("alt-2").ok()
            }
        }
        CustomAction::NewTerminalTab
        | CustomAction::NewFile
        | CustomAction::ShowAboutWarp
        | CustomAction::SplitPaneLeft
        | CustomAction::SelectAllBlocks
        | CustomAction::SplitPaneUp
        | CustomAction::ConfigureKeybindings
        | CustomAction::RenameTab
        | CustomAction::CloseTab
        | CustomAction::CloseOtherTabs
        | CustomAction::CloseTabsRight
        | CustomAction::ReferAFriend
        | CustomAction::ViewSharedBlocks
        | CustomAction::ShowAccount
        | CustomAction::ShowAppearance
        | CustomAction::SaveCurrentConfig
        | CustomAction::TriggerWelcomeBlock
        | CustomAction::HistorySearch
        | CustomAction::DisableSyncTerminalInputs
        | CustomAction::ToggleSyncAllTerminalInputsInAllTabs
        | CustomAction::NewPersonalWorkflow
        | CustomAction::NewPersonalNotebook
        | CustomAction::NewPersonalEnvVars
        | CustomAction::NewTeamWorkflow
        | CustomAction::NewTeamNotebook
        | CustomAction::NewTeamEnvVars
        | CustomAction::SearchDrive
        | CustomAction::OpenTeamSettings
        | CustomAction::ShareCurrentSession
        | CustomAction::SharePaneContents
        | CustomAction::OpenAIFactCollection
        | CustomAction::OpenMCPServerCollection
        | CustomAction::NewPersonalAIPrompt
        | CustomAction::NewTeamAIPrompt
        | CustomAction::NewAgentTab => None,
    }
}
pub fn cmd_or_ctrl_shift(key: &str) -> String {
    if OperatingSystem::get().is_mac() {
        format!("cmd-{key}")
    } else {
        let key = if Keystroke::is_valid_special_key(key) {
            // Valid keys don't need to be uppercase (we don't want to create a binding that looks
            // like `ctrl-shift-ENTER`).
            Cow::Borrowed(key)
        } else {
            if cfg!(debug_assertions) {
                let stroke = key.chars().next().expect("Character should exist");

                if !stroke.is_ascii_lowercase() {
                    panic!(
                        "Tried to register a ctrl-shift-{key} shortcut which is invalid because the {key} character needs to be modified by the shift character."
                    );
                }
            }
            // The need to uppercase the key because of the addition of the `shift`.
            // Keystroke::parse debug asserts if this the modifier is lowercase:
            // https://github.com/warpdotdev/warp-internal/blob/c225b8cedd94fdba33e957cf1efb99d84768d193/ui/src/keymap.rs#L637/
            key.to_ascii_uppercase().into()
        };
        format!("ctrl-shift-{key}")
    }
}
