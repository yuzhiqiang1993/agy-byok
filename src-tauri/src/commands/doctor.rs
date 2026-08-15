use crate::doctor::{run_auto_fix, run_diagnosis, DoctorReport, FixAction};
use crate::state::DesktopState;
use tauri::State;

#[tauri::command]
pub async fn run_doctor_diagnosis(state: State<'_, DesktopState>) -> Result<DoctorReport, String> {
    Ok(run_diagnosis(&state).await)
}

#[tauri::command]
pub async fn run_doctor_auto_fix(
    state: State<'_, DesktopState>,
    action: FixAction,
) -> Result<DoctorReport, String> {
    run_auto_fix(&state, action).await
}
