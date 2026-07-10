//! 전송·정리 UI(분해 5/8): 전송 다이얼로그(#26 리네임·#27 태그 분기)·폴더 자동 분류(#34)·
//! 진행/결과 모달(#35)과 그 상태 타입. app.rs에서 순수 이동 — 동작 변경 없음.

use super::*;

// 리네임 모드(#26)는 마지막 사용 옵션 저장(#57)을 위해 core config로 이동.
use rawblow_core::config::{OrganizeDefaults, RenameMode, TransferDefaults};

#[derive(Clone)]
pub(super) struct TransferDialogState {
    labels: Vec<Label>,
    /// 전송 대상 별점 집합(1~5). 라벨과 합집합(OR)으로 묶인다(#23).
    stars: Vec<u8>,
    /// 전송 대상 컬러 태그 집합(#27). 라벨·별점과 합집합(OR).
    tags: Vec<ColorTag>,
    action: Action,
    companions: Companions,
    split_by_label: bool,
    /// 태그별 하위폴더 분기(#27).
    split_by_tag: bool,
    conflict: ConflictPolicy,
    dest: String,
    /// 파일명 변경(#26).
    rename_mode: RenameMode,
    rename_template: String,
    rename_numbering: Numbering,
    /// 전송 대상 범위(#68): true=전체 items, false=현재 필터(라벨·별점·태그 AND) 통과분만.
    /// #57 방식으로 저장·복원(기본 전체). AI 컬링의 scope_all과 같은 개념.
    scope_all: bool,
    /// Move 원본 이동 확인 오버레이 표시 여부(#63). 세션 한정 UI 상태 — 저장하지 않는다
    /// (to_defaults에 넣지 않으므로 #57 기본값 복원 대상이 아님).
    confirm_move: bool,
}

impl TransferDialogState {
    /// 저장된 마지막 사용 옵션으로 초기화(#57). dest는 호출부가 현재 폴더 기준으로 채운다.
    fn from_defaults(d: &TransferDefaults) -> Self {
        TransferDialogState {
            labels: d.labels.clone(),
            stars: d.stars.clone(),
            tags: d.tags.clone(),
            action: d.action,
            companions: d.companions,
            split_by_label: d.split_by_label,
            split_by_tag: d.split_by_tag,
            conflict: d.conflict,
            dest: String::new(),
            rename_mode: d.rename_mode,
            rename_template: d.rename_template.clone(),
            rename_numbering: d.rename_numbering,
            scope_all: d.scope_all,
            confirm_move: false, // 확인 오버레이는 항상 닫힌 상태로 시작(비저장, #63).
        }
    }

    /// 저장용 마지막 사용 옵션(#57). dest(폴더 종속)는 제외한다.
    fn to_defaults(&self) -> TransferDefaults {
        TransferDefaults {
            labels: self.labels.clone(),
            stars: self.stars.clone(),
            tags: self.tags.clone(),
            action: self.action,
            companions: self.companions,
            split_by_label: self.split_by_label,
            split_by_tag: self.split_by_tag,
            conflict: self.conflict,
            rename_mode: self.rename_mode,
            rename_template: self.rename_template.clone(),
            rename_numbering: self.rename_numbering,
            scope_all: self.scope_all,
        }
    }

    /// 다이얼로그 상태 → 전송 리네임 규칙(#26). Off면 None.
    fn rename_rule(&self) -> Option<RenameRule> {
        match self.rename_mode {
            RenameMode::Off => None,
            RenameMode::Seq => Some(RenameRule {
                template: "{seq:03}".into(),
                numbering: Numbering::Order,
            }),
            RenameMode::Grade => Some(RenameRule {
                template: "{gradeseq}".into(),
                numbering: Numbering::GradeGrouped,
            }),
            RenameMode::Custom => Some(RenameRule {
                template: self.rename_template.clone(),
                numbering: self.rename_numbering,
            }),
        }
    }
}

impl Default for TransferDialogState {
    fn default() -> Self {
        // 기본값의 단일 출처는 config::TransferDefaults(#57) — 두 곳이 어긋나지 않게 위임.
        Self::from_defaults(&TransferDefaults::default())
    }
}

/// 폴더 자동 분류 다이얼로그 상태(#34). 셀렉 전송과 별개로, 폴더 안의 사진을 기준별로
/// 하위폴더에 나눠 담는다.
#[derive(Clone)]
pub(super) struct OrganizeDialogState {
    key: OrganizeKey,
    action: Action,
    /// 분류 결과를 담을 루트(기본: 현재 폴더 — in-place로 하위폴더 생성).
    dest: String,
    conflict: ConflictPolicy,
    /// Move 원본 이동 확인 오버레이 표시 여부(#63). 세션 한정 — 저장하지 않는다(to_defaults 제외).
    confirm_move: bool,
}

impl OrganizeDialogState {
    /// 저장된 마지막 사용 옵션으로 초기화(#57). dest는 호출부가 현재 폴더로 채운다.
    fn from_defaults(d: &OrganizeDefaults) -> Self {
        OrganizeDialogState { key: d.key, action: d.action, dest: String::new(), conflict: d.conflict, confirm_move: false }
    }

    /// 저장용 마지막 사용 옵션(#57). dest(폴더 종속)는 제외한다.
    fn to_defaults(&self) -> OrganizeDefaults {
        OrganizeDefaults { key: self.key, action: self.action, conflict: self.conflict }
    }
}

impl Default for OrganizeDialogState {
    fn default() -> Self {
        // 기본값의 단일 출처는 config::OrganizeDefaults(#57) — Move 기본(#34)도 그쪽에 정의.
        Self::from_defaults(&OrganizeDefaults::default())
    }
}

/// 백그라운드 파일 작업(전송/정리)에서 메인 스레드로 보내는 메시지(#35).
pub(super) enum JobMsg {
    Progress(Progress),
    Done(TransferReport),
}

/// 완료 후 폴더를 어떻게 다시 열지(#35/#34).
#[derive(Clone, Copy)]
pub(super) enum ReopenMode {
    /// 현재 폴더를 현재 설정대로 다시 스캔(Move 전송 후 사라진 항목 정리, #24).
    Current,
    /// 분류 결과(생성된 하위폴더)를 보도록 하위 폴더 포함으로 다시 연다(#34).
    Recursive,
}

/// 진행 중인 백그라운드 파일 작업(전송/정리)(#35). 별도 스레드에서 실행하고 진행 상황을
/// 채널로 받아 프로그레스바로 표시한다 — 큰 폴더·느린 드라이브에서 UI가 멈춘 것처럼 보이지 않게.
pub(super) struct ProgressJob {
    /// 다이얼로그 제목(tr 키, 한국어 원문).
    title: &'static str,
    rx: crossbeam_channel::Receiver<JobMsg>,
    cancel: Arc<AtomicBool>,
    latest: Progress,
    /// 완료 후 폴더 재오픈 방식.
    reopen: Option<ReopenMode>,
    /// 결과 다이얼로그의 "대상 폴더 열기"에 쓸 경로.
    dest: Option<PathBuf>,
    /// 이 작업이 폴더 정리(true)인지 전송(false)인지(#63). 결과창 제목 분리에 쓴다.
    organize: bool,
}

impl RawBlowApp {
    pub(super) fn open_transfer(&mut self) {
        // 컬링 중에는 파일 이동(폴더 재스캔으로 인덱스 무효화)을 막는다.
        if self.ai_cull.is_some() {
            self.toast_info(tr(self.lang, "AI 컬링이 끝난 뒤 전송할 수 있습니다").into());
            return;
        }
        // 마지막 사용 옵션을 기본값으로 로드(#57). dest만 현재 폴더 기준으로 새로 제안.
        let mut st = TransferDialogState::from_defaults(&self.cfg.transfer_defaults);
        if let Some(folder) = &self.folder {
            st.dest = format!("{}_selected", folder.to_string_lossy());
        }
        self.transfer = Some(st);
    }

    /// 폴더 자동 분류 다이얼로그를 연다(#34). 기본 대상은 현재 폴더(in-place 하위폴더 생성).
    pub(super) fn open_organize(&mut self) {
        if self.ai_cull.is_some() {
            self.toast_info(tr(self.lang, "AI 컬링이 끝난 뒤 정리할 수 있습니다").into());
            return;
        }
        // 마지막 사용 옵션을 기본값으로 로드(#57). dest만 현재 폴더로 새로 제안.
        let mut st = OrganizeDialogState::from_defaults(&self.cfg.organize_defaults);
        if let Some(folder) = &self.folder {
            st.dest = folder.to_string_lossy().to_string();
        }
        self.organize = Some(st);
    }

    /// 전송 대상 엔트리 목록(#68). scope_all이면 전체 items, 아니면 현재 필터(라벨·별점·태그 AND)를
    /// 통과한 항목만. 미리보기 플랜과 실제 시작이 반드시 같은 집합을 쓰도록 한 곳에서 만든다.
    fn scoped_transfer_entries(&self, scope_all: bool) -> Vec<Entry> {
        if scope_all {
            self.items.iter().map(|i| i.entry.clone()).collect()
        } else {
            self.filtered().into_iter().map(|r| self.items[r].entry.clone()).collect()
        }
    }

    // ── 전송 다이얼로그 ──────────────────────────────────
    pub(super) fn ui_transfer_dialog(&mut self, ctx: &egui::Context) {
        let lang = self.lang;
        let mut st = self.transfer.clone().unwrap();
        let mut do_start = false;
        let mut do_cancel = false;
        let mut want_start = false; // 시작 요청(버튼/Enter). Move면 확인 오버레이 경유(#63).
        let mut confirm_yes = false; // 확인 오버레이 "이동 시작".
        let mut confirm_no = false; // 확인 오버레이 "돌아가기".
        // Enter로 시작(#63). typing은 대상 폴더/리네임 템플릿 TextEdit에 포커스가 있을 때
        // Enter를 시작으로 오인하지 않게 막는 가드.
        let enter = ctx.input(|i| i.key_pressed(egui::Key::Enter));
        let typing = ctx.memory(|m| m.focused().is_some());
        let tag_names: Vec<String> =
            ColorTag::ALL.iter().map(|t| self.cfg.tag_label(*t, lang)).collect();
        // 전송 범위(#68): scope_all이면 전체 items, 아니면 현재 필터(라벨·별점·태그 AND) 통과분만.
        // 칩 카운트·플랜·리네임 프리뷰·시작·Move 확인 수까지 전부 이 스코프 기준으로 계산한다.
        let total_items = self.items.len();
        let filtered_count = self.filtered().len();
        let entries = self.scoped_transfer_entries(st.scope_all);
        // 칩 카운트는 스코프 엔트리에서 직접 센다(self.counts()/star_counts()/tag_counts()는 전체 기준이라 미사용).
        let (mut pick, mut hold, mut reject, mut unrated) = (0usize, 0usize, 0usize, 0usize);
        let mut star_cnt = [0usize; 6];
        let mut tag_cnt = [0usize; 5];
        for e in &entries {
            match e.label {
                Label::Pick => pick += 1,
                Label::Hold => hold += 1,
                Label::Reject => reject += 1,
                Label::Unrated => unrated += 1,
            }
            star_cnt[e.stars.min(5) as usize] += 1;
            if let Some(i) = e.tag.index() {
                tag_cnt[i] += 1;
            }
        }

        // 뒤 화면 어둡게(dim) + 클릭 차단. Middle 레이어 → 패널 위, 카드(Foreground) 아래.
        let screen = ctx.screen_rect();
        egui::Area::new(egui::Id::new("transfer_dim"))
            .order(egui::Order::Middle)
            .fixed_pos(Pos2::ZERO)
            .show(ctx, |ui| {
                ui.painter().with_clip_rect(screen).rect_filled(screen, 0.0, Color32::from_black_alpha(180));
                let _ = ui.allocate_rect(screen, Sense::click_and_drag());
            });

        // 미리보기 계획(footer 통계). entries는 위에서 스코프 기준으로 구성됨(#68).
        let plan = transfer::plan(&TransferRequest {
            entries: &entries,
            labels: st.labels.clone(),
            stars: st.stars.clone(),
            tags: st.tags.clone(),
            action: st.action,
            companions: st.companions,
            dest: PathBuf::from(&st.dest),
            split_by_label: st.split_by_label,
            split_by_tag: st.split_by_tag,
            conflict: st.conflict,
            rename: st.rename_rule(),
        });
        let raw_n = plan.iter().filter(|(p, _, _)| rawblow_core::model::kind_of(p) == Some(rawblow_core::model::Kind::Raw)).count();
        let img_n = plan.len().saturating_sub(raw_n);
        // 시작 가능 조건: 대상 0건이 아니고, 대상 폴더가 공백이 아닐 것(#63 — 정리와 일관되게 dest 검증 추가).
        let can_start = !plan.is_empty() && !st.dest.trim().is_empty();

        // 중앙 모달 카드. 너비 660 고정이라 left를 화면중앙-330으로 두면 가로 정중앙
        // (Area::anchor는 이전 프레임 크기 기반이라 수렴이 안 돼 fixed_pos로 직접 배치).
        let card_pos = egui::Pos2::new(screen.center().x - 330.0, (screen.center().y - 300.0).max(8.0));
        egui::Area::new(egui::Id::new("transfer_card"))
            .order(egui::Order::Foreground)
            .fixed_pos(card_pos)
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(theme::BG2)
                    .stroke(Stroke::new(1.0, theme::LINE2))
                    .rounding(10.0)
                    .show(ui, |ui| {
                        ui.set_min_width(660.0);
                        ui.set_max_width(660.0);
                        // ── HEADER ──
                        egui::Frame::none()
                            .inner_margin(egui::Margin { left: 22.0, right: 22.0, top: 18.0, bottom: 14.0 })
                            .show(ui, |ui| {
                                ui.set_width(616.0);
                                ui.horizontal(|ui| {
                                    let (ir, _) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover());
                                    send_icon(ui.painter(), ir.center(), 14.0, theme::ACCENT);
                                    ui.add_space(9.0);
                                    ui.vertical(|ui| {
                                        ui.label(egui::RichText::new(tr(lang, "파일 전송")).font(prop(15.0)).color(theme::INK));
                                        ui.label(egui::RichText::new(tr(lang, "선택한 라벨·별점의 파일을 복사/이동 · RAW 페어 처리")).font(mono(10.5)).color(theme::INK3));
                                    });
                                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                                        let (xr, xresp) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::click());
                                        let c = xr.center();
                                        let col = if xresp.hovered() { theme::INK } else { theme::INK3 };
                                        ui.painter().line_segment([c + Vec2::new(-4.0, -4.0), c + Vec2::new(4.0, 4.0)], Stroke::new(1.5, col));
                                        ui.painter().line_segment([c + Vec2::new(4.0, -4.0), c + Vec2::new(-4.0, 4.0)], Stroke::new(1.5, col));
                                        if xresp.clicked() { do_cancel = true; }
                                        if xresp.hovered() { ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand); }
                                    });
                                });
                            });
                        hline_full(ui);
                        // ── BODY ── 노트북 등 낮은 해상도에서 카드가 화면을 넘으면 본문만
                        // 스크롤시켜 헤더·푸터(전송 시작 버튼)가 항상 보이게 한다(#41).
                        // 124 = 헤더(~68) + 푸터(~54) + 구분선 2.
                        let body_max = (screen.height() - card_pos.y - 8.0 - 124.0).max(140.0);
                        egui::ScrollArea::vertical()
                            .id_salt("transfer_body_scroll")
                            .max_height(body_max)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                        egui::Frame::none()
                            .inner_margin(egui::Margin::symmetric(22.0, 18.0))
                            .show(ui, |ui| {
                                ui.set_width(616.0);
                                // ── SCOPE ── 전송 대상 범위(#68): 전체 폴더 vs 현재 필터. AI 컬링과 동일한 세그먼트.
                                // 필터가 없으면 두 카운트가 같지만 특수처리하지 않는다(그대로 노출).
                                section_label(ui, tr(lang, "범위"));
                                let scope_sel = if st.scope_all { 0 } else { 1 };
                                let all_lbl = trf(lang, "{} 항목", &[&total_items.to_string()]);
                                let filt_lbl = trf(lang, "{} 항목", &[&filtered_count.to_string()]);
                                if let Some(i) = segmented(ui, &[(tr(lang, "전체"), all_lbl.as_str()), (tr(lang, "현재 필터"), filt_lbl.as_str())], scope_sel) {
                                    st.scope_all = i == 0;
                                }
                                ui.add_space(16.0);
                                section_label(ui, tr(lang, "원본 라벨"));
                                ui.horizontal_wrapped(|ui| {
                                    for (label, n) in [(Label::Pick, pick), (Label::Hold, hold), (Label::Reject, reject), (Label::Unrated, unrated)] {
                                        let on = st.labels.contains(&label);
                                        if check_chip(ui, label.name(lang), Some(n), theme::label_color(label), on) {
                                            if on { st.labels.retain(|l| *l != label); } else { st.labels.push(label); }
                                        }
                                    }
                                });
                                ui.add_space(8.0);
                                if check_chip(ui, tr(lang, "라벨별 하위폴더로 분기 (/pick, /hold …)"), None, theme::ACCENT, st.split_by_label) {
                                    st.split_by_label = !st.split_by_label;
                                }
                                ui.add_space(16.0);

                                // 별점 기준(#23): 라벨과 합집합(OR). 각 별점 칸은 독립 체크.
                                section_label(ui, tr(lang, "별점 기준"));
                                ui.horizontal_wrapped(|ui| {
                                    for n in 1..=5u8 {
                                        let on = st.stars.contains(&n);
                                        let glyph = "★".repeat(n as usize);
                                        if check_chip(ui, &glyph, Some(star_cnt[n as usize]), theme::HOLD, on) {
                                            if on {
                                                st.stars.retain(|s| *s != n);
                                            } else {
                                                st.stars.push(n);
                                            }
                                        }
                                    }
                                });
                                ui.add_space(6.0);
                                ui.label(egui::RichText::new(tr(lang, "라벨 또는 별점 중 하나라도 해당하면 전송됩니다(합집합).")).font(mono(10.0)).color(theme::INK4));
                                ui.add_space(16.0);

                                // 컬러 태그 기준(#27): 라벨·별점과 합집합(OR). 태그별 하위폴더 분기 옵션.
                                section_label(ui, tr(lang, "컬러 태그 기준"));
                                ui.horizontal_wrapped(|ui| {
                                    for (i, tag) in ColorTag::ALL.iter().enumerate() {
                                        let on = st.tags.contains(tag);
                                        let rgb = tag.color_rgb().unwrap_or([0x6b, 0x72, 0x80]);
                                        let col = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
                                        if check_chip(ui, &tag_names[i], Some(tag_cnt[i]), col, on) {
                                            if on {
                                                st.tags.retain(|t| t != tag);
                                            } else {
                                                st.tags.push(*tag);
                                            }
                                        }
                                    }
                                });
                                ui.add_space(8.0);
                                if check_chip(ui, tr(lang, "태그별 하위폴더로 분기 (@teal …)"), None, theme::ACCENT, st.split_by_tag) {
                                    st.split_by_tag = !st.split_by_tag;
                                }
                                ui.add_space(16.0);

                                section_label(ui, tr(lang, "동작"));
                                let act_sel = if st.action == Action::Copy { 0 } else { 1 };
                                if let Some(i) = segmented(ui, &[(tr(lang, "복사"), tr(lang, "원본 유지")), (tr(lang, "이동"), tr(lang, "원본 이동"))], act_sel) {
                                    st.action = if i == 0 { Action::Copy } else { Action::Move };
                                    // Copy로 되돌리면 확인 오버레이 상태를 해제(#63).
                                    if st.action == Action::Copy { st.confirm_move = false; }
                                }
                                ui.add_space(16.0);

                                section_label(ui, tr(lang, "동반 파일"));
                                let comp_sel = match st.companions { Companions::Both => 0, Companions::RawOnly => 1, Companions::ImageOnly => 2 };
                                if let Some(i) = segmented(ui, &[(tr(lang, "RAW+이미지"), tr(lang, "페어 함께")), (tr(lang, "RAW만"), tr(lang, "RAW만")), (tr(lang, "이미지만"), tr(lang, "JPG만"))], comp_sel) {
                                    st.companions = [Companions::Both, Companions::RawOnly, Companions::ImageOnly][i];
                                }
                                ui.add_space(16.0);

                                section_label(ui, tr(lang, "대상 폴더"));
                                ui.horizontal(|ui| {
                                    let rest = (ui.available_width() - 96.0).max(120.0);
                                    ui.add(egui::TextEdit::singleline(&mut st.dest).font(mono(12.0)).desired_width(rest));
                                    if toggle_btn(ui, tr(lang, "찾아보기…"), false).clicked() {
                                        if let Some(d) = rfd::FileDialog::new().pick_folder() {
                                            st.dest = d.to_string_lossy().to_string();
                                        }
                                    }
                                });
                                ui.add_space(16.0);

                                section_label(ui, tr(lang, "이름 충돌 시"));
                                let conf_sel = if st.conflict == ConflictPolicy::AutoIncrement { 0 } else { 1 };
                                if let Some(i) = segmented(ui, &[(tr(lang, "자동 일련번호"), tr(lang, "_001 접미")), (tr(lang, "건너뛰기"), tr(lang, "기존 유지"))], conf_sel) {
                                    st.conflict = if i == 0 { ConflictPolicy::AutoIncrement } else { ConflictPolicy::Skip };
                                }
                                ui.add_space(16.0);

                                // 분류 기반 파일명 변경(#26): 프리셋 + 자유 템플릿 + 라이브 프리뷰.
                                section_label(ui, tr(lang, "리네임"));
                                let modes: [(RenameMode, &str); 4] = [
                                    (RenameMode::Off, tr(lang, "원본 유지")),
                                    (RenameMode::Seq, tr(lang, "순번 (1,2,3)")),
                                    (RenameMode::Grade, tr(lang, "별점등급 (A1,B1…)")),
                                    (RenameMode::Custom, tr(lang, "직접 입력")),
                                ];
                                ui.horizontal_wrapped(|ui| {
                                    for (mode, label) in modes {
                                        if check_chip(ui, label, None, theme::ACCENT, st.rename_mode == mode) {
                                            st.rename_mode = mode;
                                        }
                                    }
                                });
                                if st.rename_mode == RenameMode::Custom {
                                    ui.add_space(6.0);
                                    ui.horizontal(|ui| {
                                        ui.add(egui::TextEdit::singleline(&mut st.rename_template).font(mono(12.0)).desired_width(330.0).hint_text("{gradeseq}_{orig}"));
                                        let num_sel = if st.rename_numbering == Numbering::Order { 0 } else { 1 };
                                        if let Some(i) = segmented(ui, &[(tr(lang, "선택순"), tr(lang, "선택 순서")), (tr(lang, "등급순"), tr(lang, "별점 등급"))], num_sel) {
                                            st.rename_numbering = if i == 0 { Numbering::Order } else { Numbering::GradeGrouped };
                                        }
                                    });
                                    ui.add_space(3.0);
                                    ui.label(egui::RichText::new("{seq} {seq:03} {grade} {gradeseq} {stars} {label} {tag} {orig}").font(mono(9.0)).color(theme::INK4));
                                }
                                if st.rename_mode != RenameMode::Off {
                                    let preview_req = TransferRequest {
                                        entries: &entries,
                                        labels: st.labels.clone(),
                                        stars: st.stars.clone(),
                                        tags: st.tags.clone(),
                                        action: st.action,
                                        companions: st.companions,
                                        dest: PathBuf::new(),
                                        split_by_label: false,
                                        split_by_tag: false,
                                        conflict: st.conflict,
                                        rename: st.rename_rule(),
                                    };
                                    let pv = transfer::rename_preview(&preview_req, 4);
                                    ui.add_space(6.0);
                                    if pv.is_empty() {
                                        ui.label(egui::RichText::new(tr(lang, "대상 없음 — 위에서 라벨·별점·태그를 선택하세요.")).font(mono(9.5)).color(theme::INK4));
                                    } else {
                                        for (old, new) in &pv {
                                            ui.label(egui::RichText::new(format!("{}  →  {}", old, new)).font(mono(9.5)).color(theme::INK3));
                                        }
                                    }
                                }
                            });
                            });
                        hline_full(ui);
                        // ── FOOTER ──
                        egui::Frame::none()
                            .fill(theme::BG1)
                            .rounding(egui::Rounding { nw: 0.0, ne: 0.0, sw: 10.0, se: 10.0 })
                            .inner_margin(egui::Margin::symmetric(22.0, 14.0))
                            .show(ui, |ui| {
                                ui.set_width(616.0);
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new(tr(lang, "전송 대상")).font(prop(10.0)).color(theme::INK3));
                                    ui.add_space(8.0);
                                    ui.label(egui::RichText::new(plan.len().to_string()).font(mono(13.0)).color(theme::ACCENT));
                                    ui.label(egui::RichText::new(tr(lang, "파일")).font(mono(10.0)).color(theme::INK3));
                                    ui.add_space(6.0);
                                    ui.label(egui::RichText::new(raw_n.to_string()).font(mono(11.0)).color(theme::INK2));
                                    ui.label(egui::RichText::new("RAW").font(mono(9.5)).color(theme::INK3));
                                    ui.add_space(4.0);
                                    ui.label(egui::RichText::new(img_n.to_string()).font(mono(11.0)).color(theme::INK2));
                                    ui.label(egui::RichText::new(tr(lang, "이미지")).font(mono(9.5)).color(theme::INK3));

                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        // 대상이 0건이거나 대상 폴더가 공백이면 시작 버튼 비활성(빈 전송·미지정 방지, #63).
                                        if ui.add_enabled(can_start, egui::Button::new(egui::RichText::new(format!("  {}  ", tr(lang, "전송 시작"))).color(Color32::from_rgb(0x0a, 0x14, 0x20))).fill(theme::ACCENT)).clicked() {
                                            want_start = true;
                                        }
                                        ui.add_space(5.0);
                                        kbd(ui, "Enter");
                                        ui.add_space(14.0);
                                        if toggle_btn(ui, tr(lang, "취소"), false).clicked() {
                                            do_cancel = true;
                                        }
                                        ui.add_space(5.0);
                                        kbd(ui, "Esc");
                                    });
                                });
                            });
                    });
            });

        // 원본 이동 확인 오버레이(#63): Move인데 아직 미확인이면 다이얼로그 위에 확인 카드를 띄운다.
        // #57로 Move가 기본값 복원될 수 있어, 토글 하나로 즉시 실행되던 걸 확인 경유로 바꾼 안전장치.
        if st.confirm_move {
            let (yes, no) = self.ui_move_confirm_overlay(ctx, screen, plan.len());
            confirm_yes = yes;
            confirm_no = no;
        }

        let esc = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        if st.confirm_move {
            // 확인 오버레이가 떠 있는 동안: Enter/이동 시작 → 실제 시작, Esc/돌아가기 → 오버레이 닫기.
            // Esc를 여기서 소비하므로 아래 다이얼로그 Esc(취소)는 !confirm_move 가드로 막힌다.
            if enter || confirm_yes {
                do_start = true;
            } else if esc || confirm_no {
                st.confirm_move = false;
            }
        } else {
            // 일반 상태: 버튼 또는 Enter(입력 포커스 아님·시작 가능)로 시작 요청.
            if want_start || (enter && !typing && can_start) {
                if st.action == Action::Move {
                    st.confirm_move = true; // 즉시 실행 대신 확인 오버레이를 띄운다(#63).
                } else {
                    do_start = true; // Copy는 종전처럼 즉시 시작.
                }
            }
            if esc {
                do_cancel = true;
            }
        }

        if do_cancel {
            self.transfer = None;
            return;
        }
        if do_start {
            self.start_transfer(&st);
        } else {
            self.transfer = Some(st);
        }
    }

    /// 원본 이동 확인 오버레이(#63, 전송·정리 공용). 다이얼로그 위에 두 번째 dim + 중앙 카드를
    /// 그린다. 반환값은 (이동 시작 클릭, 돌아가기 클릭). 순서상 이 오버레이가 다이얼로그 카드보다
    /// 나중에 그려져 그 위에 겹친다.
    fn ui_move_confirm_overlay(&self, ctx: &egui::Context, screen: Rect, n: usize) -> (bool, bool) {
        let lang = self.lang;
        let mut yes = false;
        let mut no = false;
        // 두 번째 dim 레이어(다이얼로그 카드까지 덮도록 Foreground, 카드보다 나중에 표시).
        // 확인 카드는 한 단계 위(Tooltip)에 두어, dim을 클릭해도 카드가 dim 뒤로 가려지지
        // 않게 한다(egui의 클릭 시 앞으로 올리기는 같은 Order 안에서만 동작하므로 Order를 분리).
        egui::Area::new(egui::Id::new("move_confirm_dim"))
            .order(egui::Order::Foreground)
            .fixed_pos(Pos2::ZERO)
            .show(ctx, |ui| {
                ui.painter().with_clip_rect(screen).rect_filled(screen, 0.0, Color32::from_black_alpha(180));
                let _ = ui.allocate_rect(screen, Sense::click_and_drag());
            });
        egui::Window::new("move_confirm_modal")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .fixed_size(Vec2::new(420.0, 0.0))
            .frame(modal_frame())
            .order(egui::Order::Tooltip)
            .show(ctx, |ui| {
                ui.label(egui::RichText::new(trf(lang, "원본 {}개를 이동합니다 — 원래 폴더에서 제거됩니다.", &[&n.to_string()])).font(prop(13.0)).color(theme::WARN));
                ui.add_space(14.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(egui::Button::new(egui::RichText::new(format!("  {}  ", tr(lang, "이동 시작"))).color(Color32::from_rgb(0x0a, 0x14, 0x20))).fill(theme::ACCENT)).clicked() {
                        yes = true;
                    }
                    ui.add_space(8.0);
                    if toggle_btn(ui, tr(lang, "돌아가기"), false).clicked() {
                        no = true;
                    }
                });
            });
        (yes, no)
    }

    /// 전송을 백그라운드 스레드에서 시작하고 진행 모달로 전환한다(#35). 큰 폴더에서
    /// 메인 스레드가 막히지 않게 별도 스레드에서 `execute_with_progress`를 돌리고,
    /// 진행 상황을 채널로 받는다(Move면 완료 후 폴더를 재스캔해 사라진 항목 정리, #24).
    pub(super) fn start_transfer(&mut self, st: &TransferDialogState) {
        // 마지막 사용 옵션 저장(#57) — 다음 열기 때 기본값으로 복원된다.
        self.cfg.transfer_defaults = st.to_defaults();
        let _ = config::save(&self.cfg);
        // 플랜(미리보기)과 동일한 스코프로 엔트리를 구성 — 보여준 것만 정확히 전송한다(#68).
        let entries = self.scoped_transfer_entries(st.scope_all);
        let labels = st.labels.clone();
        let stars = st.stars.clone();
        let tags = st.tags.clone();
        let action = st.action;
        let companions = st.companions;
        let dest = PathBuf::from(&st.dest);
        let split_by_label = st.split_by_label;
        let split_by_tag = st.split_by_tag;
        let conflict = st.conflict;
        let rename = st.rename_rule();

        let (tx, rx) = crossbeam_channel::unbounded::<JobMsg>();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_t = cancel.clone();
        let dest_t = dest.clone();
        std::thread::spawn(move || {
            let req = TransferRequest {
                entries: &entries,
                labels,
                stars,
                tags,
                action,
                companions,
                dest: dest_t,
                split_by_label,
                split_by_tag,
                conflict,
                rename,
            };
            let mut on = |p: &Progress| {
                let _ = tx.send(JobMsg::Progress(p.clone()));
                !cancel_t.load(Ordering::Relaxed)
            };
            let report = transfer::execute_with_progress(&req, &mut on);
            let _ = tx.send(JobMsg::Done(report));
        });

        // Move는 원본이 사라지므로 완료 후 재스캔 필요. Copy는 목록 불변이라 재오픈 불필요.
        let reopen = matches!(action, Action::Move).then_some(ReopenMode::Current);
        self.transfer = None;
        self.progress = Some(ProgressJob {
            title: "전송 중",
            rx,
            cancel,
            latest: Progress::default(),
            reopen,
            dest: Some(dest),
            organize: false,
        });
    }

    /// 폴더 자동 분류를 백그라운드 스레드에서 시작하고 진행 모달로 전환한다(#34/#35).
    /// 완료 후 분류 결과(하위폴더)를 보도록 하위 폴더 포함으로 폴더를 다시 연다.
    pub(super) fn start_organize(&mut self, st: &OrganizeDialogState) {
        // 마지막 사용 옵션 저장(#57) — 다음 열기 때 기본값으로 복원된다.
        self.cfg.organize_defaults = st.to_defaults();
        let _ = config::save(&self.cfg);
        let entries: Vec<Entry> = self.items.iter().map(|i| i.entry.clone()).collect();
        let key = st.key;
        let action = st.action;
        let dest = PathBuf::from(&st.dest);
        let conflict = st.conflict;

        let (tx, rx) = crossbeam_channel::unbounded::<JobMsg>();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_t = cancel.clone();
        let dest_t = dest.clone();
        std::thread::spawn(move || {
            let req = OrganizeRequest {
                entries: &entries,
                key,
                action,
                dest_root: dest_t,
                conflict,
            };
            let mut on = |p: &Progress| {
                let _ = tx.send(JobMsg::Progress(p.clone()));
                !cancel_t.load(Ordering::Relaxed)
            };
            let report = organize::organize_with_progress(&req, &mut on);
            let _ = tx.send(JobMsg::Done(report));
        });

        // 재오픈 방식: 현재 폴더 안에 분류(in-place)했는지로 갈린다.
        //  - Move + in-place: 루트 파일이 하위폴더로 빠짐 → 하위폴더 포함 재스캔으로 결과 표시.
        //  - Move + 외부 대상: 현재 폴더가 비워짐 → 현재 설정대로 재스캔(사라진 항목 정리).
        //  - Copy: 원본이 그대로 남아 현재 보기가 유효 → 재오픈 불필요(복사본은 하위/외부에).
        let in_place = self
            .folder
            .as_ref()
            .map(|f| dest.starts_with(f))
            .unwrap_or(false);
        let reopen = match action {
            Action::Move if in_place => Some(ReopenMode::Recursive),
            Action::Move => Some(ReopenMode::Current),
            Action::Copy => None,
        };
        self.organize = None;
        self.progress = Some(ProgressJob {
            title: "폴더 정리 중",
            rx,
            cancel,
            latest: Progress::default(),
            reopen,
            dest: Some(dest),
            organize: true,
        });
    }

    /// 진행 모달(#35): 채널을 비워 진행률을 갱신하고, 완료(Done)면 후처리(재오픈+결과)로 전환.
    pub(super) fn ui_progress(&mut self, ctx: &egui::Context) {
        let lang = self.lang;
        let mut job = self.progress.take().unwrap();

        // 채널 비우기: 진행 갱신은 마지막 값만 유지, 완료면 결과 확보.
        let mut done_report: Option<TransferReport> = None;
        let mut disconnected = false;
        loop {
            match job.rx.try_recv() {
                Ok(JobMsg::Progress(p)) => job.latest = p,
                Ok(JobMsg::Done(r)) => {
                    done_report = Some(r);
                    break;
                }
                Err(crossbeam_channel::TryRecvError::Empty) => break,
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }

        if let Some(report) = done_report {
            let reopen = job.reopen;
            let dest = job.dest.clone();
            self.progress = None;
            match reopen {
                Some(ReopenMode::Current) => {
                    if let Some(folder) = self.folder.clone() {
                        self.open_folder(folder);
                    }
                }
                Some(ReopenMode::Recursive) => {
                    // 분류 결과(하위폴더)가 보이도록 재귀 스캔으로 전환·저장 후 재오픈.
                    if report.transferred > 0 {
                        self.cfg.recursive = true;
                        let _ = config::save(&self.cfg);
                    }
                    if let Some(folder) = self.folder.clone() {
                        self.open_folder(folder);
                    }
                }
                None => {}
            }
            self.last_dest = dest;
            self.result_organize = job.organize; // 결과창 제목 분리(전송/정리)(#63).
            self.result = Some(report);
            return;
        }
        if disconnected {
            // 작업 스레드가 Done 없이 종료(패닉 등, #63). 무음으로 닫지 않고 실패 리포트를
            // 띄워 작업이 죽었음을 알린다 — 사용자가 결과를 오해하지 않게.
            self.progress = None;
            let report = TransferReport {
                failed: vec![(PathBuf::from("-"), tr(lang, "작업이 예기치 않게 중단되었습니다").to_string())],
                ..Default::default()
            };
            self.result_organize = job.organize;
            self.last_dest = job.dest.clone();
            self.result = Some(report);
            return;
        }

        // 작업이 도는 동안 결과 채널을 꾸준히 펌프(워커는 egui를 깨우지 못함).
        ctx.request_repaint_after(Duration::from_millis(80));

        let p = &job.latest;
        let frac = if p.total > 0 {
            (p.done as f32 / p.total as f32).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let mut do_cancel = false;

        // 뒤 화면 어둡게 + 클릭 차단.
        let screen = ctx.screen_rect();
        egui::Area::new(egui::Id::new("progress_dim"))
            .order(egui::Order::Middle)
            .fixed_pos(Pos2::ZERO)
            .show(ctx, |ui| {
                ui.painter().with_clip_rect(screen).rect_filled(screen, 0.0, Color32::from_black_alpha(180));
                let _ = ui.allocate_rect(screen, Sense::click_and_drag());
            });

        egui::Window::new("progress_modal")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .fixed_size(Vec2::new(480.0, 0.0))
            .frame(modal_frame())
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                modal_header(ui, tr(lang, job.title), "");
                ui.add(egui::ProgressBar::new(frac).fill(theme::ACCENT).desired_height(10.0));
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new(trf(lang, "{} / {} 파일", &[&p.done.to_string(), &p.total.to_string()]))
                        .font(mono(12.0))
                        .color(theme::INK),
                );
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new(format!("{:.1} MB", p.bytes as f64 / 1_048_576.0))
                        .font(mono(10.5))
                        .color(theme::INK3),
                );
                if !p.current.is_empty() {
                    ui.add_space(2.0);
                    ui.label(egui::RichText::new(&p.current).font(mono(10.0)).color(theme::INK4));
                }
                ui.add_space(14.0);
                let r = ui.max_rect();
                let y = ui.cursor().top();
                ui.painter().hline(r.left()..=r.right(), y, Stroke::new(1.0, theme::LINE));
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if toggle_btn(ui, tr(lang, "취소"), false).clicked() {
                            do_cancel = true;
                        }
                    });
                });
            });

        if do_cancel || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            // 취소 신호만 보내고 모달은 유지 — 작업 스레드가 다음 파일 직전에 멈춰 Done을 보낸다.
            job.cancel.store(true, Ordering::Relaxed);
        }
        self.progress = Some(job);
    }

    pub(super) fn ui_transfer_result(&mut self, ctx: &egui::Context) {
        let lang = self.lang;
        let report = self.result.clone().unwrap();
        let mut close = false;
        let mut open_dest = false;

        // 뒤 화면 어둡게 + 클릭 차단(#72). 결과는 명시적으로 닫혀야 하므로 dim을
        // 클릭해도 닫히지 않는다 — 클릭 통과만 막는다.
        let screen = ctx.screen_rect();
        egui::Area::new(egui::Id::new("result_dim"))
            .order(egui::Order::Middle)
            .fixed_pos(Pos2::ZERO)
            .show(ctx, |ui| {
                ui.painter().with_clip_rect(screen).rect_filled(screen, 0.0, Color32::from_black_alpha(180));
                let _ = ui.allocate_rect(screen, Sense::click_and_drag());
            });

        egui::Window::new("transfer_result")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .fixed_size(Vec2::new(560.0, 0.0))
            .frame(modal_frame())
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                // 정리(#34) 결과면 제목을 분리해 표시(#63).
                let title = if self.result_organize { tr(lang, "정리 완료") } else { tr(lang, "전송 완료") };
                modal_header(ui, title, "");
                if report.canceled {
                    ui.label(egui::RichText::new(tr(lang, "취소됨")).font(prop(12.0)).color(theme::WARN));
                    ui.add_space(4.0);
                }
                ui.label(egui::RichText::new(trf(lang, "✓ {} 파일 전송 · {} 리네임 · {} 실패", &[&report.transferred.to_string(), &report.renamed.len().to_string(), &report.failed.len().to_string()])).font(prop(13.0)).color(theme::OK));
                ui.add_space(6.0);
                ui.label(egui::RichText::new(trf(lang, "RAW {} · 이미지 {} · {:.1} MB", &[&report.raw_count.to_string(), &report.image_count.to_string(), &format!("{:.1}", report.bytes as f64 / 1_048_576.0)])).font(mono(11.0)).color(theme::INK2));
                // 동명 파일 존재로 건너뛴 수(#63): 조용히 사라지지 않게 명시한다.
                if report.skipped > 0 {
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(trf(lang, "건너뜀 {} — 동명 파일 존재", &[&report.skipped.to_string()])).font(mono(11.0)).color(theme::WARN));
                }
                if !report.renamed.is_empty() {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(format!("{} · {}", tr(lang, "이름 변경"), report.renamed.len())).font(prop(10.0)).color(theme::WARN));
                    for (a, b) in report.renamed.iter().take(10) {
                        ui.label(egui::RichText::new(format!("{a} → {b}")).font(mono(10.5)).color(theme::INK3));
                    }
                }
                if !report.failed.is_empty() {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(format!("{} · {}", tr(lang, "실패 목록"), report.failed.len())).font(prop(10.0)).color(theme::REJECT));
                    for (p, e) in report.failed.iter().take(5) {
                        ui.label(egui::RichText::new(format!("{} — {e}", p.display())).font(mono(10.0)).color(theme::INK3));
                    }
                }
                // 이동 후 원본 삭제 실패(#63): 전송은 됐지만 원본이 남았음을 경고로 알리고 경로를 나열.
                if !report.remove_failed.is_empty() {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(trf(lang, "원본 삭제 실패 {} — 원본 파일이 남아 있습니다", &[&report.remove_failed.len().to_string()])).font(prop(10.0)).color(theme::WARN));
                    for p in report.remove_failed.iter().take(3) {
                        ui.label(egui::RichText::new(p.display().to_string()).font(mono(10.0)).color(theme::INK3));
                    }
                }
                ui.add_space(14.0);
                let r = ui.max_rect();
                let y = ui.cursor().top();
                ui.painter().hline(r.left()..=r.right(), y, Stroke::new(1.0, theme::LINE));
                ui.add_space(12.0);
                let has_dest = self.last_dest.as_ref().map(|d| d.is_dir()).unwrap_or(false);
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(egui::Button::new(egui::RichText::new(format!("  {}  ", tr(lang, "닫기"))).color(Color32::from_rgb(0x0a, 0x14, 0x20))).fill(theme::ACCENT)).clicked() {
                            close = true;
                        }
                        ui.add_space(8.0);
                        if has_dest && toggle_btn(ui, tr(lang, "대상 폴더 열기"), false).clicked() {
                            open_dest = true;
                        }
                    });
                });
            });
        if open_dest {
            if let Some(dest) = self.last_dest.clone() {
                if dest.is_dir() {
                    // 전송 결과의 "대상 폴더 열기"는 OS 파일 탐색기를 띄운다(Finder/Explorer).
                    // 과거에는 RawBlow가 작업 폴더를 그 경로로 전환했는데, macOS에서 이
                    // 경로가 강제종료를 일으켰고 UX 의도와도 맞지 않았다(#5).
                    reveal_in_file_manager(&dest);
                }
            }
            self.result = None;
            return;
        }
        if close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.result = None;
        }
    }

    /// 폴더 자동 분류 다이얼로그(#34). 셀렉 전송과 별개로 폴더 안 사진을 기준별 하위폴더로 정리.
    pub(super) fn ui_organize_dialog(&mut self, ctx: &egui::Context) {
        let lang = self.lang;
        let mut st = self.organize.clone().unwrap();
        let mut do_start = false;
        let mut do_cancel = false;
        let mut want_start = false; // 시작 요청(버튼/Enter). Move면 확인 오버레이 경유(#63).
        let mut confirm_yes = false; // 확인 오버레이 "이동 시작".
        let mut confirm_no = false; // 확인 오버레이 "돌아가기".
        // Enter로 시작(#63). typing은 대상 폴더 TextEdit 포커스 중 Enter 오인을 막는 가드.
        let enter = ctx.input(|i| i.key_pressed(egui::Key::Enter));
        let typing = ctx.memory(|m| m.focused().is_some());

        // 대상 카운트(가벼움). 확장자 기준은 폴더 분포도 즉석 계산(EXIF 불필요).
        let file_count: usize = self.items.iter().map(|i| i.entry.members.len()).sum();
        // 시작 가능: 대상 파일이 있고 대상 폴더가 공백이 아닐 것(정리는 기존부터 dest 검증).
        let can_start = file_count > 0 && !st.dest.trim().is_empty();
        let ext_breakdown: Vec<(String, usize)> = if st.key == OrganizeKey::Extension {
            use std::collections::BTreeMap;
            let mut m: BTreeMap<String, usize> = BTreeMap::new();
            for it in &self.items {
                for src in &it.entry.members {
                    let e = src
                        .extension()
                        .and_then(|s| s.to_str())
                        .map(|s| s.to_ascii_uppercase())
                        .unwrap_or_else(|| "—".into());
                    *m.entry(e).or_default() += 1;
                }
            }
            m.into_iter().collect()
        } else {
            Vec::new()
        };

        // 뒤 화면 어둡게 + 클릭 차단.
        let screen = ctx.screen_rect();
        egui::Area::new(egui::Id::new("organize_dim"))
            .order(egui::Order::Middle)
            .fixed_pos(Pos2::ZERO)
            .show(ctx, |ui| {
                ui.painter().with_clip_rect(screen).rect_filled(screen, 0.0, Color32::from_black_alpha(180));
                let _ = ui.allocate_rect(screen, Sense::click_and_drag());
            });

        egui::Window::new("organize_modal")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .fixed_size(Vec2::new(520.0, 0.0))
            .frame(modal_frame())
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                modal_header(
                    ui,
                    tr(lang, "폴더 자동 분류"),
                    tr(lang, "폴더 안 사진을 기준별 하위폴더로 정리 · 셀렉 전송과 별개"),
                );

                section_label(ui, tr(lang, "기준"));
                let keys: [(OrganizeKey, &str); 4] = [
                    (OrganizeKey::Date, tr(lang, "촬영일")),
                    (OrganizeKey::Camera, tr(lang, "카메라")),
                    (OrganizeKey::Lens, tr(lang, "렌즈")),
                    (OrganizeKey::Extension, tr(lang, "확장자")),
                ];
                ui.horizontal_wrapped(|ui| {
                    for (k, label) in keys {
                        if check_chip(ui, label, None, theme::ACCENT, st.key == k) {
                            st.key = k;
                        }
                    }
                });
                ui.add_space(16.0);

                section_label(ui, tr(lang, "동작"));
                let act_sel = if st.action == Action::Copy { 0 } else { 1 };
                if let Some(i) = segmented(ui, &[(tr(lang, "복사"), tr(lang, "원본 유지")), (tr(lang, "이동"), tr(lang, "원본 이동"))], act_sel) {
                    st.action = if i == 0 { Action::Copy } else { Action::Move };
                    // Copy로 되돌리면 확인 오버레이 상태를 해제(#63).
                    if st.action == Action::Copy { st.confirm_move = false; }
                }
                ui.add_space(16.0);

                section_label(ui, tr(lang, "대상 폴더"));
                ui.horizontal(|ui| {
                    let rest = (ui.available_width() - 96.0).max(120.0);
                    ui.add(egui::TextEdit::singleline(&mut st.dest).font(mono(12.0)).desired_width(rest));
                    if toggle_btn(ui, tr(lang, "찾아보기…"), false).clicked() {
                        if let Some(d) = rfd::FileDialog::new().pick_folder() {
                            st.dest = d.to_string_lossy().to_string();
                        }
                    }
                });
                ui.add_space(4.0);
                ui.label(egui::RichText::new(tr(lang, "대상 폴더 안에 기준별 하위폴더가 생성됩니다.")).font(mono(10.0)).color(theme::INK4));
                ui.add_space(16.0);

                section_label(ui, tr(lang, "이름 충돌 시"));
                let conf_sel = if st.conflict == ConflictPolicy::AutoIncrement { 0 } else { 1 };
                if let Some(i) = segmented(ui, &[(tr(lang, "자동 일련번호"), tr(lang, "_001 접미")), (tr(lang, "건너뛰기"), tr(lang, "기존 유지"))], conf_sel) {
                    st.conflict = if i == 0 { ConflictPolicy::AutoIncrement } else { ConflictPolicy::Skip };
                }
                ui.add_space(16.0);

                // 미리보기/안내: 확장자는 즉석 폴더 분포, EXIF 기준은 실행 중 분류 안내.
                if st.key == OrganizeKey::Extension && !ext_breakdown.is_empty() {
                    section_label(ui, tr(lang, "미리보기"));
                    for (folder, n) in ext_breakdown.iter().take(6) {
                        ui.label(egui::RichText::new(format!("{}/  ·  {}", folder, n)).font(mono(10.5)).color(theme::INK3));
                    }
                    if ext_breakdown.len() > 6 {
                        ui.label(egui::RichText::new(format!("… +{}", ext_breakdown.len() - 6)).font(mono(10.0)).color(theme::INK4));
                    }
                } else if st.key != OrganizeKey::Extension {
                    ui.label(egui::RichText::new(tr(lang, "촬영일·카메라·렌즈 기준은 실행하며 EXIF를 읽어 분류합니다. RAW+JPG 페어는 같은 폴더로 유지됩니다.")).font(mono(10.0)).color(theme::INK4));
                }

                ui.add_space(14.0);
                let r = ui.max_rect();
                let y = ui.cursor().top();
                ui.painter().hline(r.left()..=r.right(), y, Stroke::new(1.0, theme::LINE));
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(tr(lang, "정리 대상")).font(prop(10.0)).color(theme::INK3));
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(file_count.to_string()).font(mono(13.0)).color(theme::ACCENT));
                    ui.label(egui::RichText::new(tr(lang, "파일")).font(mono(10.0)).color(theme::INK3));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add_enabled(can_start, egui::Button::new(egui::RichText::new(format!("  {}  ", tr(lang, "정리 시작"))).color(Color32::from_rgb(0x0a, 0x14, 0x20))).fill(theme::ACCENT)).clicked() {
                            want_start = true;
                        }
                        ui.add_space(8.0);
                        if toggle_btn(ui, tr(lang, "취소"), false).clicked() {
                            do_cancel = true;
                        }
                    });
                });
            });

        // 원본 이동 확인 오버레이(#63): Move인데 아직 미확인이면 정리 다이얼로그 위에 확인 카드를 띄운다.
        if st.confirm_move {
            let (yes, no) = self.ui_move_confirm_overlay(ctx, screen, file_count);
            confirm_yes = yes;
            confirm_no = no;
        }

        let esc = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        if st.confirm_move {
            // 오버레이가 떠 있으면 Enter/이동 시작 → 실제 시작, Esc/돌아가기 → 닫기(Esc 소비).
            if enter || confirm_yes {
                do_start = true;
            } else if esc || confirm_no {
                st.confirm_move = false;
            }
        } else {
            if want_start || (enter && !typing && can_start) {
                if st.action == Action::Move {
                    st.confirm_move = true; // 즉시 실행 대신 확인 오버레이(#63).
                } else {
                    do_start = true;
                }
            }
            if esc {
                do_cancel = true;
            }
        }

        if do_cancel {
            self.organize = None;
            return;
        }
        if do_start {
            self.start_organize(&st);
        } else {
            self.organize = Some(st);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rename_rule_off_is_none() {
        // 원본 유지 모드는 리네임 규칙 없음.
        let st = TransferDialogState { rename_mode: RenameMode::Off, ..Default::default() };
        assert!(st.rename_rule().is_none());
    }

    #[test]
    fn rename_rule_seq_uses_order_numbering() {
        // 순번 프리셋: {seq:03} + 선택 순서대로.
        let st = TransferDialogState { rename_mode: RenameMode::Seq, ..Default::default() };
        let rule = st.rename_rule().expect("Seq는 Some");
        assert_eq!(rule.template, "{seq:03}");
        assert_eq!(rule.numbering, Numbering::Order);
    }

    #[test]
    fn rename_rule_grade_uses_grade_grouped_numbering() {
        // 별점등급 프리셋: {gradeseq} + 등급별 묶음 순번.
        let st = TransferDialogState { rename_mode: RenameMode::Grade, ..Default::default() };
        let rule = st.rename_rule().expect("Grade는 Some");
        assert_eq!(rule.template, "{gradeseq}");
        assert_eq!(rule.numbering, Numbering::GradeGrouped);
    }

    #[test]
    fn rename_rule_custom_passes_through_state_verbatim() {
        // 직접 입력 모드는 다이얼로그의 템플릿·번호방식을 그대로 통과시킨다.
        let st = TransferDialogState {
            rename_mode: RenameMode::Custom,
            rename_template: "{label}_{orig}_shot".into(),
            rename_numbering: Numbering::Order, // 기본값(GradeGrouped)과 다른 값으로 통과 확인
            ..Default::default()
        };
        let rule = st.rename_rule().expect("Custom은 Some");
        assert_eq!(rule.template, "{label}_{orig}_shot");
        assert_eq!(rule.numbering, Numbering::Order);
    }

    #[test]
    fn transfer_dialog_state_defaults() {
        let st = TransferDialogState::default();
        assert_eq!(st.labels, vec![Label::Pick]);
        assert!(st.stars.is_empty());
        assert!(st.tags.is_empty());
        assert_eq!(st.action, Action::Copy);
        assert_eq!(st.companions, Companions::Both);
        assert!(!st.split_by_label);
        assert!(!st.split_by_tag);
        assert_eq!(st.conflict, ConflictPolicy::AutoIncrement);
        assert!(matches!(st.rename_mode, RenameMode::Off)); // RenameMode는 Debug 미구현이라 matches! 사용
        assert!(st.scope_all); // 전송 범위 기본값은 전체(#68)
    }

    #[test]
    fn transfer_defaults_round_trip_excludes_dest() {
        // #57: 옵션은 저장·복원되지만 dest(폴더 종속)는 매번 새로 제안하므로 저장하지 않는다.
        let st = TransferDialogState {
            action: Action::Move,
            split_by_tag: true,
            rename_mode: RenameMode::Custom,
            rename_template: "{orig}_pick".into(),
            rename_numbering: Numbering::Order,
            scope_all: false, // 기본(true)과 다른 값으로 저장·복원 확인(#68)
            dest: r"X:\old\folder_selected".into(),
            ..Default::default()
        };
        let st2 = TransferDialogState::from_defaults(&st.to_defaults());
        assert_eq!(st2.action, Action::Move);
        assert!(st2.split_by_tag);
        assert_eq!(st2.rename_mode, RenameMode::Custom);
        assert_eq!(st2.rename_template, "{orig}_pick");
        assert_eq!(st2.rename_numbering, Numbering::Order);
        assert!(!st2.scope_all); // 전송 범위도 저장·복원(#68)
        assert!(st2.dest.is_empty()); // dest는 복원 대상 아님
    }

    #[test]
    fn organize_defaults_round_trip_excludes_dest() {
        // #57 정리 다이얼로그도 동일: key/action/conflict만 저장·복원.
        let st = OrganizeDialogState {
            key: OrganizeKey::Lens,
            action: Action::Copy,
            conflict: ConflictPolicy::Skip,
            dest: r"X:\old\folder".into(),
            confirm_move: false,
        };
        let st2 = OrganizeDialogState::from_defaults(&st.to_defaults());
        assert_eq!(st2.key, OrganizeKey::Lens);
        assert_eq!(st2.action, Action::Copy);
        assert_eq!(st2.conflict, ConflictPolicy::Skip);
        assert!(st2.dest.is_empty());
    }

    #[test]
    fn organize_dialog_state_defaults_to_move_unlike_transfer_copy() {
        // 이슈 #34 의도: 전송(Copy 기본)과 달리 폴더 정리는 Move가 기본.
        let st = OrganizeDialogState::default();
        assert_eq!(st.key, OrganizeKey::Date);
        assert_eq!(st.action, Action::Move);
        assert_eq!(st.conflict, ConflictPolicy::AutoIncrement);
        assert_ne!(st.action, TransferDialogState::default().action);
    }
}
