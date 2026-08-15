import { t } from '../i18n';
import { runDoctorAutoFix, runDoctorDiagnosis } from '../controllers/doctorController';
import { refreshHostStatuses } from '../controllers/hostController';
import { proxyService } from '../services/proxyService';
import { configService } from '../services/configService';
import { store } from '../store/appStore';
import { switchTab } from './TabManager';
import { openProviderEditor } from './ProviderEditor';
import type { DiagnosticCategory, DiagnosticLevel, DoctorReport, FixAction } from '../types/doctor';
import { createModal, type ModalInstance } from './common/Modal';
import { showNotice } from './NoticeBar';

let currentModal: ModalInstance | null = null;

async function syncGlobalAppState(): Promise<void> {
  try {
    const [proxyResult, configResult] = await Promise.allSettled([
      proxyService.getStatus(),
      configService.getConfig(),
      refreshHostStatuses(),
    ]);
    if (proxyResult.status === 'fulfilled') {
      store.setProxyStatus(proxyResult.value);
    }
    if (configResult.status === 'fulfilled') {
      store.setConfig(configResult.value);
    }
  } catch (e) {
    console.error('Failed to sync global app state after doctor action:', e);
  }
}

function categoryIconSvg(category: DiagnosticCategory): string {
  switch (category) {
    case 'proxy':
      return `<svg class="doctor-cat-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"></polygon></svg>`;
    case 'config':
      return `<svg class="doctor-cat-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16z"></path></svg>`;
    case 'provider':
      return `<svg class="doctor-cat-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"></circle><line x1="2" y1="12" x2="22" y2="12"></line><path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z"></path></svg>`;
    case 'host':
      return `<svg class="doctor-cat-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="20" height="14" x="2" y="3" rx="2"></rect><line x1="8" y1="21" x2="16" y2="21"></line><line x1="12" y1="17" x2="12" y2="21"></line></svg>`;
  }
}

function categoryLabel(category: DiagnosticCategory): string {
  switch (category) {
    case 'proxy':
      return t('doctor.categoryProxy');
    case 'config':
      return t('doctor.categoryConfig');
    case 'provider':
      return t('doctor.categoryProvider');
    case 'host':
      return t('doctor.categoryHost');
  }
}

function statusPill(level: DiagnosticLevel): HTMLElement {
  const pill = document.createElement('span');
  pill.className = `doctor-status-pill doctor-status-${level}`;
  if (level === 'pass') {
    pill.textContent = t('doctor.badgePass');
  } else if (level === 'info') {
    pill.textContent = t('doctor.badgeInfo');
  } else if (level === 'warning') {
    pill.textContent = t('doctor.badgeWarning');
  } else {
    pill.textContent = t('doctor.badgeError');
  }
  return pill;
}

function statusDot(level: DiagnosticLevel): HTMLElement {
  const dot = document.createElement('span');
  dot.className = `doctor-dot doctor-dot-${level}`;
  return dot;
}

function getActionLabel(action: FixAction): string {
  switch (action.type) {
    case 'start_proxy':
      return t('doctor.btnStartProxy');
    case 'open_add_provider':
      return t('doctor.btnGoConfigure');
    case 'restart_app_host':
      return t('doctor.btnRestartApp');
    case 'restart_ide_host':
      return t('doctor.btnRestartIde');
    case 'prune_invalid_models':
      return t('doctor.btnPruneModels');
    case 'repair_ide_settings':
    case 'repair_app_environment':
      return t('doctor.btnRepairSettings');
    default:
      return t('doctor.autoFix');
  }
}

function getActionLoadingLabel(action: FixAction): string {
  switch (action.type) {
    case 'start_proxy':
      return t('doctor.starting');
    case 'restart_app_host':
    case 'restart_ide_host':
      return t('doctor.restarting');
    case 'prune_invalid_models':
      return t('doctor.pruning');
    default:
      return t('doctor.fixing');
  }
}

export function openDoctorModal(): void {
  currentModal?.close();

  const body = document.createElement('div');
  body.className = 'doctor-modal-body';

  const loadingEl = document.createElement('div');
  loadingEl.className = 'doctor-loading';
  loadingEl.innerHTML = `
    <div class="doctor-spinner"></div>
    <span>${t('doctor.running')}</span>
  `;
  body.append(loadingEl);

  const renderReport = (report: DoctorReport) => {
    body.innerHTML = '';

    // 统计各状态数量
    const totalCount = report.items.length;
    const passCount = report.items.filter((i) => i.level === 'pass' || i.level === 'info').length;
    const issueCount = totalCount - passCount;
    const issueText = issueCount > 0 ? t('doctor.issuesCount', { count: issueCount }) : '';
    const statsText = t('doctor.summaryStats', { total: totalCount, passed: passCount, issueText });

    // 1. 顶部 Header 状态横幅
    const banner = document.createElement('div');
    banner.className = `doctor-banner doctor-banner-${report.overallStatus}`;

    const bannerIcon = document.createElement('div');
    bannerIcon.className = 'doctor-banner-icon';
    if (report.overallStatus === 'pass') {
      bannerIcon.innerHTML = `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"></path><path d="m9 12 2 2 4-4"></path></svg>`;
    } else if (report.overallStatus === 'warning') {
      bannerIcon.innerHTML = `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3Z"></path><line x1="12" y1="9" x2="12" y2="13"></line><line x1="12" y1="17" x2="12.01" y2="17"></line></svg>`;
    } else {
      bannerIcon.innerHTML = `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"></circle><line x1="15" y1="9" x2="9" y2="15"></line><line x1="9" y1="9" x2="15" y2="15"></line></svg>`;
    }

    const bannerContent = document.createElement('div');
    bannerContent.className = 'doctor-banner-content';

    const bannerTitle = document.createElement('div');
    bannerTitle.className = 'doctor-banner-title';
    if (report.overallStatus === 'pass') {
      bannerTitle.textContent = t('doctor.overallPass');
    } else if (report.overallStatus === 'warning') {
      bannerTitle.textContent = t('doctor.overallWarning');
    } else {
      bannerTitle.textContent = t('doctor.overallError');
    }

    const bannerStats = document.createElement('div');
    bannerStats.className = 'doctor-banner-stats';
    bannerStats.textContent = statsText;

    bannerContent.append(bannerTitle, bannerStats);

    const bannerMeta = document.createElement('div');
    bannerMeta.className = 'doctor-banner-meta';
    bannerMeta.textContent = t('doctor.checkTime', { time: new Date(report.timestampMs).toLocaleTimeString() });

    banner.append(bannerIcon, bannerContent, bannerMeta);
    body.append(banner);

    // 2. 按分类整齐呈现分组列表
    const categories: DiagnosticCategory[] = ['proxy', 'config', 'provider', 'host'];
    for (const cat of categories) {
      const items = report.items.filter((i) => i.category === cat);
      if (items.length === 0) continue;

      const groupCard = document.createElement('div');
      groupCard.className = 'doctor-card-group';

      const groupHeader = document.createElement('div');
      groupHeader.className = 'doctor-card-group-header';
      groupHeader.innerHTML = `
        <div class="doctor-card-group-title">
          ${categoryIconSvg(cat)}
          <span>${categoryLabel(cat)}</span>
        </div>
      `;
      groupCard.append(groupHeader);

      const rowList = document.createElement('div');
      rowList.className = 'doctor-row-list';

      for (const item of items) {
        const row = document.createElement('div');
        row.className = `doctor-row doctor-row-${item.level}`;

        // 主体信息行
        const rowMain = document.createElement('div');
        rowMain.className = 'doctor-row-main';

        const rowLeft = document.createElement('div');
        rowLeft.className = 'doctor-row-left';
        rowLeft.append(statusDot(item.level));

        const textCol = document.createElement('div');
        textCol.className = 'doctor-text-col';

        const titleEl = document.createElement('div');
        titleEl.className = 'doctor-item-title';
        if (item.title.includes('（未接入代理）')) {
          const parts = item.title.split('（未接入代理）');
          titleEl.innerHTML = `${parts[0]}<span class="doctor-title-warn-tag">（未接入代理）</span>${parts[1] || ''}`;
        } else if (item.title.includes('(未接入代理)')) {
          const parts = item.title.split('(未接入代理)');
          titleEl.innerHTML = `${parts[0]}<span class="doctor-title-warn-tag">(未接入代理)</span>${parts[1] || ''}`;
        } else {
          titleEl.textContent = item.title;
        }

        const msgEl = document.createElement('div');
        msgEl.className = 'doctor-item-msg';
        msgEl.textContent = item.message;

        textCol.append(titleEl, msgEl);
        rowLeft.append(textCol);

        const rowRight = document.createElement('div');
        rowRight.className = 'doctor-row-right';

        // 若可一键操作，直接在右侧展示操作按钮替代单纯的异常/提示 tag；否则展示对应状态 tag
        if (item.autoFixable && item.action) {
          const fixBtn = document.createElement('button');
          fixBtn.className = 'doctor-action-btn';
          const actionLabel = getActionLabel(item.action);
          const iconSvg = item.action.type === 'open_add_provider'
            ? `<svg class="doctor-fix-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"></path><polyline points="15 3 21 3 21 9"></polyline><line x1="10" y1="14" x2="21" y2="3"></line></svg>`
            : `<svg class="doctor-fix-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z"></path></svg>`;
          fixBtn.innerHTML = `
            ${iconSvg}
            <span>${actionLabel}</span>
          `;
          fixBtn.addEventListener('click', async () => {
            if (item.action?.type === 'open_add_provider') {
              currentModal?.close();
              await switchTab('tab-models');
              void openProviderEditor();
              return;
            }
            fixBtn.disabled = true;
            fixBtn.classList.add('loading');
            const loadingText = getActionLoadingLabel(item.action as FixAction);
            fixBtn.innerHTML = `<span>${loadingText}</span>`;
            try {
              const updated = await runDoctorAutoFix(item.action as FixAction);
              showNotice(t('doctor.fixSuccess'), 'success');
              renderReport(updated);
              void syncGlobalAppState();
            } catch (err: any) {
              showNotice(t('doctor.fixFailed', { message: err?.message || String(err) }), 'error');
              fixBtn.disabled = false;
              fixBtn.classList.remove('loading');
              fixBtn.innerHTML = `<span>${actionLabel}</span>`;
            }
          });
          rowRight.append(fixBtn);
        } else {
          rowRight.append(statusPill(item.level));
        }

        rowMain.append(rowLeft, rowRight);
        row.append(rowMain);

        // 如果不可自动修复但有手动指引建议，才展示微型提示行
        if (!item.autoFixable && item.suggestion) {
          const hintBar = document.createElement('div');
          hintBar.className = `doctor-hint-bar doctor-hint-${item.level}`;
          hintBar.innerHTML = `<span class="doctor-hint-icon">💡</span> <span>${t('doctor.suggestion', { text: item.suggestion })}</span>`;
          row.append(hintBar);
        }

        rowList.append(row);
      }

      groupCard.append(rowList);
      body.append(groupCard);
    }
  };

  currentModal = createModal({
    title: t('doctor.title'),
    subtitle: t('doctor.subtitle'),
    body,
    dialogClassName: 'doctor-modal-dialog',
    okLabel: t('doctor.btnRecheck'),
    cancelLabel: t('modal.close'),
    onOk: async () => {
      body.innerHTML = '';
      body.append(loadingEl);
      try {
        const report = await runDoctorDiagnosis();
        renderReport(report);
        void syncGlobalAppState();
      } catch (err: any) {
        showNotice(t('doctor.diagnosisFailed', { message: err?.message || String(err) }), 'error');
      }
    },
    onClosed: () => {
      currentModal = null;
      void syncGlobalAppState();
    },
  });

  // 初始加载诊断报告
  runDoctorDiagnosis()
    .then((report) => renderReport(report))
    .catch((err) => {
      body.innerHTML = '';
      const errEl = document.createElement('div');
      errEl.className = 'doctor-error';
      errEl.textContent = t('doctor.diagnosisFailed', { message: err?.message || String(err) });
      body.append(errEl);
    });
}
