import { doctorService } from '../services/doctorService';
import type { DoctorReport, FixAction } from '../types/doctor';

export async function runDoctorDiagnosis(): Promise<DoctorReport> {
  return doctorService.runDiagnosis();
}

export async function runDoctorAutoFix(action: FixAction): Promise<DoctorReport> {
  return doctorService.runAutoFix(action);
}
