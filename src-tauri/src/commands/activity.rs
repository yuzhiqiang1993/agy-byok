use crate::state::DesktopState;
use agy_byok::proxy::ActivityItem;
use tauri::State;

#[tauri::command]
pub(crate) fn get_activity_log(state: State<'_, DesktopState>) -> Vec<ActivityItem> {
    state.activity_log.get_recent()
}

#[tauri::command]
pub(crate) fn clear_activity_log(state: State<'_, DesktopState>) {
    state.activity_log.clear();
}
