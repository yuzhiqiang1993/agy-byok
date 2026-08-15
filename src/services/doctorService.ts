import { invoke } from '@tauri-apps/api/core';
import type { DoctorReport, FixAction } from '../types/doctor';

export const doctorService = {
  runDiagnosis: (): Promise<DoctorReport> =>
    invoke<DoctorReport>('run_doctor_diagnosis'),

  runAutoFix: (action: FixAction): Promise<DoctorReport> =>
    invoke<DoctorReport>('run_doctor_auto_fix', { action }),
};
