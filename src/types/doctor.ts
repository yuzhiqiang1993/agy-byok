export type DiagnosticLevel = 'pass' | 'info' | 'warning' | 'error';

export type DiagnosticCategory = 'proxy' | 'config' | 'provider' | 'host';

export type FixAction =
  | { type: 'start_proxy' }
  | { type: 'open_add_provider' }
  | { type: 'repair_ide_settings' }
  | { type: 'repair_app_environment' }
  | { type: 'restart_app_host' }
  | { type: 'restart_ide_host' }
  | {
      type: 'prune_invalid_models';
      provider_id: string;
      invalid_model_ids: string[];
    }
  | {
      type: 'enable_host_integration';
      host_type: 'ide' | 'app' | 'cli';
    };

export interface DiagnosticItem {
  id: string;
  category: DiagnosticCategory;
  title: string;
  message: string;
  suggestion?: string | null;
  level: DiagnosticLevel;
  autoFixable: boolean;
  action?: FixAction | null;
}

export interface DoctorReport {
  timestampMs: number;
  overallStatus: DiagnosticLevel;
  items: DiagnosticItem[];
}
