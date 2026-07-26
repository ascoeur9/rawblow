//! 상시 화면(분해 8/8): Studio 셸(툴바·좌측 레일·필름스트립·상태바)·단일뷰·그리드·
//! 사진 뷰(줌/팬·HUD·AF/지도 오버레이)·전체화면. app.rs에서 순수 이동 — 동작 변경 없음.

use super::*;

impl RawBlowApp {
    // ── Studio 셸: 툴바 + 좌측레일 + 필름스트립 + 상태바 + 중앙 ──
    /// 사진 표시 화면(매팅) 배경색(#36). 설정값이 있으면 그 색, 없으면 앱 기본(near-black void).
    pub(super) fn photo_bg(&self) -> Color32 {
        match self.cfg.photo_bg {
            Some([r, g, b]) => Color32::from_rgb(r, g, b),
            None => theme::BG0,
        }
    }

    /// 현재 사진 배경 RGB(설정값 또는 기본) — 설정의 HEX/RGB 편집 시작값(#36).
    pub(super) fn photo_bg_rgb(&self) -> [u8; 3] {
        self.cfg.photo_bg.unwrap_or(theme::BG0_RGB)
    }

    /// 스트립·그리드 셀 표기(선택 표시·별점·색상 태그)의 크기 배율(#44). 설정에 따라 크게(1.8)/작게(1.0).
    pub(super) fn badge_scale(&self) -> f32 {
        if self.cfg.large_badges {
            1.8
        } else {
            1.0
        }
    }


    pub(super) fn ui_shell(&mut self, ctx: &egui::Context) {
        self.ui_toolbar(ctx);
        self.ui_status_bar(ctx);
        self.ui_left_rail(ctx);
        if self.view == ViewMode::Single {
            self.ui_filmstrip(ctx);
        }
        let bg = self.photo_bg();
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(bg))
            .show(ctx, |ui| match self.view {
                ViewMode::Single => self.ui_single(ui),
                ViewMode::Grid => self.ui_grid(ui),
            });
    }

    pub(super) fn ui_toolbar(&mut self, ctx: &egui::Context) {
        let lang = self.lang;
        egui::TopBottomPanel::top("toolbar")
            .exact_height(TOOLBAR_H)
            .frame(egui::Frame::none().fill(theme::BG1).inner_margin(egui::Margin::symmetric(12.0, 6.0)))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    // 로고 마크(작게).
                    let (mr, _) = ui.allocate_exact_size(Vec2::new(22.0, 22.0), Sense::hover());
                    crate::logo::draw_mark(ui.painter(), mr);
                    ui.add_space(6.0);
                    vsep(ui);
                    // 폴더 열기(이슈 #2: 폴더 연 상태에서 다른 폴더 여는 버튼 없음 — ⌘O를 못 찾음).
                    if toggle_btn(ui, &format!("{} ({}O)", tr(lang, "폴더 열기"), cmd_key()), false).clicked() {
                        self.pick_folder();
                    }
                    vsep(ui);
                    let single = self.view == ViewMode::Single;
                    // "(T)"만으로는 토글임이 드러나지 않아 툴팁으로 보완(#72).
                    if toggle_btn(ui, "Single (T)", single).on_hover_text(tr(lang, "T 키로 전환")).clicked() {
                        self.view = ViewMode::Single;
                    }
                    if toggle_btn(ui, "Grid (T)", !single).on_hover_text(tr(lang, "T 키로 전환")).clicked() {
                        self.view = ViewMode::Grid;
                    }
                    vsep(ui);

                    if ui.add(egui::Button::new("◀").frame(false)).clicked() {
                        self.advance(-1);
                    }
                    let f = self.filtered();
                    let pos = format!("{:03} / {}", (self.index + 1).min(f.len().max(1)), f.len());
                    ui.label(egui::RichText::new(pos).font(mono(12.0)).color(theme::INK).background_color(theme::BG2));
                    if ui.add(egui::Button::new("▶").frame(false)).clicked() {
                        self.advance(1);
                    }
                    vsep(ui);

                    if toggle_btn(ui, "Fit (Space)", self.fit).clicked() {
                        self.fit = true;
                        self.pan = Vec2::ZERO;
                    }
                    if toggle_btn(ui, "1:1 (Space)", !self.fit && (self.zoom - 1.0).abs() < 0.01).clicked() {
                        self.fit = false;
                        self.zoom = 1.0;
                        self.pan = Vec2::ZERO;
                    }
                    if toggle_btn(ui, "ORIG (D)", self.full_raw).clicked() {
                        self.full_raw = !self.full_raw;
                    }
                    // 전체화면 토글(#31). 버튼/F11 어느 쪽이든 OS 창 풀스크린으로(update에서 동기화).
                    if toggle_btn(ui, "Full (F11)", self.fullscreen).clicked() {
                        self.fullscreen = !self.fullscreen;
                    }
                    vsep(ui);
                    if toggle_btn(ui, "EXIF (I)", self.show_exif).clicked() {
                        self.show_exif = !self.show_exif;
                    }
                    if toggle_btn(ui, "Hist (H)", self.show_hist).clicked() {
                        self.show_hist = !self.show_hist;
                    }
                    // (필터 변경은 좌측 레일에서 전담 — 상단 Filter 버튼 제거, #toolbar feedback)

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(egui::Button::new(egui::RichText::new(format!(" {} ({}E) ", tr(lang, "전송"), cmd_key())).color(Color32::from_rgb(0x0a, 0x14, 0x20))).fill(theme::ACCENT))
                            .clicked()
                        {
                            self.open_transfer();
                        }
                        // 폴더 자동 분류(#34)는 좌측 레일 하단으로 이동(테스트 피드백) — 여긴 전송/점프/일괄만.
                        if toggle_btn(ui, &format!("{} (G)", tr(lang, "점프")), false).clicked() {
                            self.jump_open = true;
                        }
                        // 그리드 모드 한정: 파일명으로 일괄 라벨링(#3).
                        if self.view == ViewMode::Grid
                            && toggle_btn(ui, &format!("{} (B)", tr(lang, "일괄")), false).clicked()
                        {
                            self.bulk_open = true;
                            self.bulk_searched = false;
                            self.bulk_hits.clear();
                        }
                        // 아이콘만으로는 기능이 드러나지 않아 툴팁으로 보완(#72).
                        if toggle_btn(ui, "⚙", self.show_settings).on_hover_text(tr(lang, "설정")).clicked() {
                            self.show_settings = true;
                            self.cache_size = None; // 설정 열 때 캐시 용량 새로 계산.
                            self.bg_hex = hex_str(self.photo_bg_rgb()); // 배경 HEX 입력 버퍼 동기화(#36).
                            self.settings_reset_armed = false; // '기본값 복원' 확인 arm 해제(#69).
                        }
                        // 단축키 치트시트(#66): ⚙ 옆 작은 ? 토글(⚙과 같은 스타일). ?/F1로도 여닫는다.
                        if toggle_btn(ui, "?", self.show_help).on_hover_text(tr(lang, "단축키")).clicked() {
                            self.show_help = !self.show_help;
                        }
                    });
                });
            });
    }

    pub(super) fn ui_left_rail(&mut self, ctx: &egui::Context) {
        let lang = self.lang;
        let (pick, hold, reject, unrated) = self.counts();
        let total = self.items.len().max(1);
        egui::SidePanel::left("rail")
            .exact_width(RAIL_W)
            .resizable(false)
            .frame(egui::Frame::none().fill(theme::BG1))
            .show(ctx, |ui| {
                section_head(ui, tr(lang, "분류"), Some("Q W E R"));
                // 레일 라벨명은 아래 필터 행(Label::name)과 동일 출처로 통일(#73): 이중 표기 제거.
                // 대문자화로 기존 시각 스타일(PICK/HOLD/…) 유지 — 한/일은 대소문자 없어 그대로 표시.
                let rows = [
                    (Label::Pick, pick, "Q"),
                    (Label::Hold, hold, "W"),
                    (Label::Reject, reject, "E"),
                    (Label::Unrated, unrated, "R"),
                ];
                let cur_label = self.current_real().and_then(|r| self.items.get(r)).map(|i| i.entry.label);
                for (label, n, key) in rows {
                    let active = cur_label == Some(label) && !matches!(label, Label::Unrated);
                    let resp = ui.allocate_response(Vec2::new(RAIL_W - 16.0, 30.0), Sense::click());
                    let rect = resp.rect;
                    let p = ui.painter();
                    if active {
                        p.rect(rect, Rounding::same(5.0), theme::BG2, Stroke::new(1.0_f32, widgets::with_alpha(theme::label_color(label), 64)));
                    }
                    p.circle_filled(Pos2::new(rect.left() + 12.0, rect.center().y), 4.0, theme::label_color(label));
                    p.text(Pos2::new(rect.left() + 26.0, rect.center().y), Align2::LEFT_CENTER, label.name(lang).to_uppercase(), prop(11.5), theme::INK2);
                    p.text(Pos2::new(rect.right() - 36.0, rect.center().y), Align2::RIGHT_CENTER, n.to_string(), mono(11.0), theme::INK);
                    p.text(Pos2::new(rect.right() - 10.0, rect.center().y), Align2::RIGHT_CENTER, key, mono(10.0), theme::INK3);
                    // 클릭 가능함을 알리는 포인터 커서(#72) — 별점·태그 칩과 동일하게.
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if resp.clicked() {
                        self.set_label(label);
                    }
                }

                // ── Rating (별점, #23) ── 라벨과 독립. 현재 항목 별점을 1~5로 지정/해제.
                section_head(ui, tr(lang, "별점"), Some("1–5 · `"));
                let cur_stars = self
                    .current_real()
                    .and_then(|r| self.items.get(r))
                    .map(|i| i.entry.stars)
                    .unwrap_or(0);
                let mut clicked_star: Option<u8> = None;
                ui.horizontal(|ui| {
                    ui.add_space(10.0);
                    ui.spacing_mut().item_spacing.x = 1.0;
                    for n in 1..=5u8 {
                        let (r, resp) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::click());
                        let filled = n <= cur_stars;
                        ui.painter().text(
                            r.center(),
                            Align2::CENTER_CENTER,
                            if filled { "★" } else { "☆" },
                            prop(16.0),
                            if filled { theme::HOLD } else { theme::INK4 },
                        );
                        if resp.clicked() {
                            clicked_star = Some(n);
                        }
                        if resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                    }
                    ui.add_space(8.0);
                    let (cr, cresp) = ui.allocate_exact_size(Vec2::new(28.0, 22.0), Sense::click());
                    ui.painter().text(
                        cr.center(),
                        Align2::CENTER_CENTER,
                        tr(lang, "해제"),
                        prop(10.5),
                        if cresp.hovered() { theme::INK } else { theme::INK3 },
                    );
                    if cresp.clicked() {
                        clicked_star = Some(0);
                    }
                    if cresp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                });
                if let Some(n) = clicked_star {
                    // 레일에서 마우스로 별을 클릭할 땐 사진을 넘기지 않는다(방금 매긴 컷을 계속 보게).
                    self.set_stars(n, false);
                }

                // ── Color tag (#27) ── 라벨·별점과 독립. 현재 항목에 5색 중 하나 부여/해제(⇧1~5).
                section_head(ui, tr(lang, "색"), Some("⇧1–5"));
                let cur_tag = self
                    .current_real()
                    .and_then(|r| self.items.get(r))
                    .map(|i| i.entry.tag)
                    .unwrap_or(ColorTag::None);
                let tag_cnt_in = self.tag_counts();
                let tag_names: Vec<String> =
                    ColorTag::ALL.iter().map(|t| self.cfg.tag_label(*t, lang)).collect();
                let mut clicked_tag: Option<ColorTag> = None;
                ui.horizontal(|ui| {
                    ui.add_space(10.0);
                    ui.spacing_mut().item_spacing.x = 4.0;
                    for (i, tag) in ColorTag::ALL.iter().enumerate() {
                        let rgb = tag.color_rgb().unwrap_or([0x6b, 0x72, 0x80]);
                        let col = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
                        let active = cur_tag == *tag;
                        let (r, resp) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::click());
                        ui.painter().circle_filled(r.center(), if active { 9.0 } else { 6.5 }, col);
                        if active {
                            ui.painter().circle_stroke(r.center(), 10.0, Stroke::new(1.5_f32, theme::INK));
                        }
                        if resp.clicked() {
                            clicked_tag = Some(*tag);
                        }
                        if resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        resp.on_hover_text(format!("{} · {}", tag_names[i], tag_cnt_in[i]));
                    }
                    ui.add_space(6.0);
                    let (cr, cresp) = ui.allocate_exact_size(Vec2::new(28.0, 22.0), Sense::click());
                    ui.painter().text(
                        cr.center(),
                        Align2::CENTER_CENTER,
                        tr(lang, "해제"),
                        prop(10.5),
                        if cresp.hovered() { theme::INK } else { theme::INK3 },
                    );
                    if cresp.clicked() {
                        clicked_tag = Some(ColorTag::None);
                    }
                    if cresp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                });
                if let Some(t) = clicked_tag {
                    self.set_tag(t);
                }

                section_head(ui, tr(lang, "진행"), None);
                ui.horizontal(|ui| {
                    ui.add_space(14.0);
                    let labeled = pick + hold + reject;
                    let pct = labeled as f32 / total as f32 * 100.0;
                    ui.label(egui::RichText::new(format!("{:.1}%", pct)).font(mono(10.0)).color(theme::INK3));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_space(14.0);
                        ui.label(egui::RichText::new(format!("{} / {}", labeled, total)).font(mono(10.0)).color(theme::INK3));
                    });
                });

                section_head(ui, tr(lang, "보기 필터"), None);
                // 필터 초기화 어포던스(#67): 세 축(라벨·별점·태그) 중 하나라도 걸려 있을 때만 노출.
                if self.any_filter_active() {
                    let resp = ui.allocate_response(Vec2::new(RAIL_W - 16.0, 18.0), Sense::click());
                    let rect = resp.rect;
                    ui.painter().text(
                        Pos2::new(rect.left() + 12.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        tr(lang, "필터 초기화"),
                        prop(10.5),
                        if resp.hovered() { theme::INK } else { theme::INK3 },
                    );
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if resp.clicked() {
                        self.reset_filters();
                    }
                }
                for filt in [Filter::All, Filter::Pick, Filter::Hold, Filter::Reject, Filter::Unrated] {
                    let active = self.filter == filt;
                    let resp = ui.allocate_response(Vec2::new(RAIL_W - 16.0, 24.0), Sense::click());
                    let rect = resp.rect;
                    let p = ui.painter();
                    if active {
                        p.rect_filled(rect, Rounding::same(4.0), theme::BG3);
                    }
                    p.text(Pos2::new(rect.left() + 12.0, rect.center().y), Align2::LEFT_CENTER, filt.name(lang), prop(12.0), if active { theme::INK } else { theme::INK2 });
                    // 클릭 가능함을 알리는 포인터 커서(#72) — 별점·태그 칩과 동일하게.
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if resp.clicked() {
                        self.filter = filt;
                        self.index = 0;
                    }
                }

                // 별점 필터(#23 후속): 라벨 필터와 독립 AND. 정확히 N점만 표시. `전체`=별점 무시.
                section_head(ui, tr(lang, "별점 필터"), None);
                let star_sel = self.star_filter;
                let star_cnt = self.star_counts(); // [미부여, 1★ .. 5★]
                let mut new_star: Option<StarFilter> = None;
                ui.horizontal(|ui| {
                    ui.add_space(10.0);
                    ui.spacing_mut().item_spacing.x = 3.0;
                    // 전체(Any)
                    let active = star_sel == StarFilter::Any;
                    let (r, resp) = ui.allocate_exact_size(Vec2::new(30.0, 22.0), Sense::click());
                    if active {
                        ui.painter().rect_filled(r, Rounding::same(4.0), theme::BG3);
                    }
                    ui.painter().text(r.center(), Align2::CENTER_CENTER, tr(lang, "전체"), prop(11.0), if active { theme::INK } else { theme::INK2 });
                    if resp.clicked() {
                        new_star = Some(StarFilter::Any);
                    }
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    // ★1 ~ ★5 (정확히 n점). 해당 점수 항목이 없으면 흐리게.
                    for n in 1..=5u8 {
                        let active = star_sel == StarFilter::Exact(n);
                        let empty = star_cnt[n as usize] == 0;
                        let (r, resp) = ui.allocate_exact_size(Vec2::new(22.0, 22.0), Sense::click());
                        if active {
                            ui.painter().rect_filled(r, Rounding::same(4.0), theme::BG3);
                        }
                        let col = if active {
                            theme::HOLD
                        } else if empty {
                            theme::INK4
                        } else {
                            theme::INK2
                        };
                        ui.painter().text(r.center(), Align2::CENTER_CENTER, format!("★{n}"), prop(10.5), col);
                        if resp.clicked() {
                            new_star = Some(StarFilter::Exact(n));
                        }
                        if resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                    }
                });
                if let Some(sf) = new_star {
                    // 같은 별점 칩 재클릭 = 토글 해제(전체로). 그 외엔 해당 별점만.
                    self.star_filter = if sf == self.star_filter { StarFilter::Any } else { sf };
                    self.index = 0;
                }

                // 컬러 태그 필터(#27): 라벨·별점 필터와 독립 AND. 특정 색만 표시. `전체`=태그 무시.
                section_head(ui, tr(lang, "색 필터"), None);
                let tag_sel = self.tag_filter;
                let tcnt = self.tag_counts();
                let mut new_tag_filter: Option<TagFilter> = None;
                ui.horizontal(|ui| {
                    ui.add_space(10.0);
                    ui.spacing_mut().item_spacing.x = 3.0;
                    let active = tag_sel == TagFilter::Any;
                    let (r, resp) = ui.allocate_exact_size(Vec2::new(30.0, 22.0), Sense::click());
                    if active {
                        ui.painter().rect_filled(r, Rounding::same(4.0), theme::BG3);
                    }
                    ui.painter().text(r.center(), Align2::CENTER_CENTER, tr(lang, "전체"), prop(11.0), if active { theme::INK } else { theme::INK2 });
                    if resp.clicked() {
                        new_tag_filter = Some(TagFilter::Any);
                    }
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    for (i, tag) in ColorTag::ALL.iter().enumerate() {
                        let active = tag_sel == TagFilter::Only(*tag);
                        let empty = tcnt[i] == 0;
                        let (r, resp) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::click());
                        if active {
                            ui.painter().rect_filled(r, Rounding::same(4.0), theme::BG3);
                        }
                        let rgb = tag.color_rgb().unwrap_or([0x6b, 0x72, 0x80]);
                        let mut col = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
                        if empty && !active {
                            col = widgets::with_alpha(col, 64);
                        }
                        ui.painter().circle_filled(r.center(), 6.5, col);
                        if resp.clicked() {
                            new_tag_filter =
                                Some(if active { TagFilter::Any } else { TagFilter::Only(*tag) });
                        }
                        if resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                    }
                });
                if let Some(tf) = new_tag_filter {
                    self.tag_filter = tf;
                    self.index = 0;
                }

                ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                    // 폴더 자동 분류(#34): 툴바에서 레일 하단으로 이동(테스트 피드백). 풀폭 버튼.
                    // 자동저장(saved/SESSION) 상태표시는 하단 상태바로 이동 — 여기선 제거.
                    // 폴더 아이콘은 벡터(컬러)로 직접 그린다 — 이모지(🗂)가 폰트에서 두부(□)로 깨져서(#33 후속).
                    ui.add_space(10.0); // 레일 하단 여백(bottom_up이라 버튼 아래쪽 패딩).
                    // 컬링 진행 중에는 폴더를 바꾸는 무거운 작업(정리)을 잠가 인덱스 안정성을 지킨다.
                    let org_enabled = !self.items.is_empty() && self.ai_cull.is_none();
                    // 폭은 항상 사이드바 가용 너비에 맞춘다 — 환경(스크롤바 등)에 따라 좌측으로
                    // 쏠리던 문제 방지. 내용(아이콘+글자)은 셀 중앙 기준이라 자동으로 가운데 정렬.
                    let (org_rect, org_resp) = ui.allocate_exact_size(
                        Vec2::new(ui.available_width(), 32.0),
                        if org_enabled { Sense::click() } else { Sense::hover() },
                    );
                    {
                        let p = ui.painter();
                        let txt_col = if org_enabled { theme::INK } else { theme::INK4 };
                        let fill = if org_enabled && org_resp.hovered() { theme::BG4 } else { theme::BG3 };
                        p.rect(org_rect, Rounding::same(6.0), fill, Stroke::new(1.0_f32, theme::LINE2));
                        // 아이콘 + 글자 묶음을 셀 가운데에 배치(총 너비 계산 후 시작 x 산출).
                        let icon = 16.0;
                        let gap = 7.0;
                        let font = prop(12.0);
                        let label = tr(lang, "정리");
                        let galley = p.layout_no_wrap(label.to_string(), font.clone(), txt_col);
                        let x0 = org_rect.center().x - (icon + gap + galley.size().x) / 2.0;
                        let icon_rect = Rect::from_min_size(
                            Pos2::new(x0, org_rect.center().y - icon / 2.0),
                            Vec2::splat(icon),
                        );
                        widgets::draw_folder_icon(p, icon_rect, org_enabled);
                        p.text(Pos2::new(x0 + icon + gap, org_rect.center().y), Align2::LEFT_CENTER, label, font, txt_col);
                    }
                    if org_enabled {
                        if org_resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if org_resp.clicked() {
                            self.open_organize();
                        }
                    }

                    // AI 컬링(#50): 정리 버튼 **바로 위**(bottom_up이라 코드상 뒤가 위). 강조색 테두리로
                    // 일반 '정리'와 구분. 항목이 있을 때만 활성. 진행 중에는 버튼이 프로그레스바로 바뀌고
                    // 클릭하면 취소 확인창을 띄운다.
                    ui.add_space(8.0);
                    let cull_progress = self.ai_cull.as_ref().map(|j| (j.progress.load(Ordering::Relaxed), j.total));
                    if let Some((done, total)) = cull_progress {
                        // 진행 중: 프로그레스바 + "AI 분석 nn%" — 클릭 시 취소 확인.
                        let (ai_rect, ai_resp) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), 32.0),
                            Sense::click(),
                        );
                        let frac = if total > 0 { (done as f32 / total as f32).clamp(0.0, 1.0) } else { 0.0 };
                        {
                            let p = ui.painter();
                            // 외곽선 없이 — 진행률은 채움(ACCENT_DIM)으로 표현.
                            p.rect(ai_rect, Rounding::same(6.0), theme::BG3, Stroke::NONE);
                            // 채움(진행률) — 좌측에서 frac 비율만큼.
                            if frac > 0.0 {
                                let fill_rect = Rect::from_min_size(
                                    ai_rect.min,
                                    Vec2::new(ai_rect.width() * frac, ai_rect.height()),
                                );
                                p.rect_filled(fill_rect, Rounding::same(6.0), theme::ACCENT_DIM);
                            }
                            let label = format!("✨ {} {}%", tr(lang, "분석"), (frac * 100.0).round() as i32);
                            p.text(ai_rect.center(), Align2::CENTER_CENTER, label, prop(12.0), theme::ACCENT);
                        }
                        if ai_resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if ai_resp.clicked() {
                            self.ai_cull_cancel_confirm = true;
                        }
                    } else {
                        let ai_enabled = !self.items.is_empty();
                        let (ai_rect, ai_resp) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), 32.0),
                            if ai_enabled { Sense::click() } else { Sense::hover() },
                        );
                        {
                            let p = ui.painter();
                            let txt_col = if ai_enabled { theme::ACCENT } else { theme::INK4 };
                            let fill = if ai_enabled && ai_resp.hovered() { theme::BG4 } else { theme::BG3 };
                            // 외곽선 없이 채움만(강조 테두리가 거슬린다는 피드백). 강조는 텍스트 색으로.
                            p.rect(ai_rect, Rounding::same(6.0), fill, Stroke::NONE);
                            let label = format!("✨ {}", tr(lang, "AI 컬링"));
                            p.text(ai_rect.center(), Align2::CENTER_CENTER, label, prop(12.0), txt_col);
                        }
                        if ai_enabled {
                            if ai_resp.hovered() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                            if ai_resp.clicked() {
                                self.ai_cull_open = true;
                            }
                        }
                    }

                    // 새 릴리즈 안내(#33): 정리 버튼 **위에** 강조 배너(bottom_up이라 코드상 뒤가 위).
                    // 3줄 — 컬러 스파클 아이콘 / "새로운 버전이 있습니다" / 버전. 각각 한 줄씩 가운데 정렬.
                    // 아이콘은 벡터(컬러)로 직접 그림(🎉 이모지 두부 회피). 클릭 시 Releases 열고 배너 닫음.
                    if let Some(ver) = self.update_available.clone() {
                        ui.add_space(8.0);
                        let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 80.0), Sense::click());
                        // ✕ dismiss 히트 영역(우상단, #69): 배너 본문과 겹치되 별개 rect·id로 상호작용을
                        // 분리한다. 여기 클릭은 브라우저를 열지 않고 이번 세션 동안만 배너를 닫는다.
                        let x_rect = Rect::from_min_size(Pos2::new(rect.right() - 22.0, rect.top() + 4.0), Vec2::splat(18.0));
                        let x_resp = ui.interact(x_rect, ui.id().with("update_dismiss"), Sense::click());
                        {
                            let p = ui.painter();
                            let dark = Color32::from_rgb(0x0a, 0x14, 0x20);
                            let fill = if resp.hovered() { Color32::from_rgb(0x8e, 0xc9, 0xff) } else { theme::ACCENT };
                            p.rect(rect, Rounding::same(8.0), fill, Stroke::new(1.0_f32, theme::ACCENT));
                            // 1줄: 컬러 스파클 아이콘(가운데 상단).
                            let icon = 22.0;
                            let icon_rect = Rect::from_center_size(
                                Pos2::new(rect.center().x, rect.top() + 6.0 + icon / 2.0),
                                Vec2::splat(icon),
                            );
                            widgets::draw_sparkles(p, icon_rect);
                            // 2줄: 안내 문구.
                            p.text(
                                Pos2::new(rect.center().x, rect.top() + 42.0),
                                Align2::CENTER_CENTER,
                                tr(lang, "새로운 버전이 있습니다"),
                                prop(12.0),
                                dark,
                            );
                            // 3줄: 버전.
                            p.text(
                                Pos2::new(rect.center().x, rect.top() + 62.0),
                                Align2::CENTER_CENTER,
                                &ver,
                                prop(12.5),
                                dark,
                            );
                            // 우상단 ✕(hover 시 밝게). 배너 텍스트와 같은 dark 톤을 기본으로.
                            p.text(
                                x_rect.center(),
                                Align2::CENTER_CENTER,
                                "✕",
                                prop(12.0),
                                if x_resp.hovered() { theme::INK } else { dark },
                            );
                        }
                        if x_resp.hovered() || resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        // ✕를 먼저 확인 — 겹친 배너 본문 클릭이 함께 발동해 브라우저가 열리지 않도록(#69).
                        if x_resp.clicked() {
                            self.update_available = None;
                        } else if resp.clicked() {
                            open_url("https://github.com/ascoeur9/rawblow/releases/latest");
                            self.update_available = None;
                        }
                    }
                });
            });
    }

    pub(super) fn ui_status_bar(&mut self, ctx: &egui::Context) {
        let (pick, hold, reject, unrated) = self.counts();
        let f_len = self.filtered().len();
        let fps = if self.frame_ms > 0.0 { 1000.0 / self.frame_ms } else { 0.0 };
        egui::TopBottomPanel::bottom("status")
            .exact_height(STATUS_H)
            .frame(egui::Frame::none().fill(theme::BG2).inner_margin(egui::Margin::symmetric(12.0, 4.0)))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(egui::RichText::new("●").color(theme::OK).size(8.0));
                    ui.label(egui::RichText::new(format!("READY · {} ITEMS · {}/{}", self.items.len(), (self.index + 1).min(f_len.max(1)), f_len)).font(mono(10.5)).color(theme::INK3));
                    ui.label(egui::RichText::new(format!("· P {pick} H {hold} X {reject} · {unrated}")).font(mono(10.5)).color(theme::INK3));
                    // 로딩·프레임 통계: 우측 → 좌측으로 이동(테스트 피드백).
                    ui.label(egui::RichText::new(format!("· {:.1}ms · {:.0} FPS · GPU wgpu · PRELOAD ±{}", self.frame_ms, fps, self.cfg.preload)).font(mono(10.5)).color(theme::INK4));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        // 맨 오른쪽: 자동저장 상태(레일에서 이동). right_to_left이므로 먼저 추가 = 최우측.
                        // 글자(우) 왼쪽에 상태 점을 둬 "● saved"/"● saving…"로 보이게 한다.
                        // 저장 실패(#62)가 최우선: 실패 중에도 dirty가 유지된 채 재시도하므로
                        // "saving…"으로 보이면 거짓 안심이다. 원인은 hover로(상태바 폭 유지).
                        let (dot, txt) = if self.save_error.is_some() {
                            (theme::REJECT, tr(self.lang, "저장 실패"))
                        } else if self.sidecar_dirty {
                            (theme::HOLD, "saving…")
                        } else {
                            (theme::OK, "saved")
                        };
                        let resp = ui.label(egui::RichText::new(txt).font(mono(10.5)).color(theme::INK3));
                        if let Some(err) = &self.save_error {
                            // 원인(원본 오류 문자열)과 대상 폴더를 hover에 — 진단은 여기서.
                            let dir = self.folder.as_deref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
                            resp.on_hover_text(format!("{dir}\n{err}"));
                        }
                        ui.label(egui::RichText::new("●").color(dot).size(8.0));
                        // 그 왼쪽: 배율(단일/전체화면에서만 의미).
                        if self.view == ViewMode::Single || self.fullscreen {
                            ui.label(egui::RichText::new("·").font(mono(10.5)).color(theme::INK4));
                            ui.label(
                                egui::RichText::new(format!("{:.0}%", self.zoom * 100.0))
                                    .font(mono(10.5))
                                    .color(theme::INK2),
                            );
                        }
                    });
                });
            });
    }

    pub(super) fn ui_filmstrip(&mut self, ctx: &egui::Context) {
        let f = self.filtered();
        egui::TopBottomPanel::bottom("filmstrip")
            .exact_height(FILMSTRIP_H)
            .frame(egui::Frame::none().fill(theme::BG1).inner_margin(egui::Margin::symmetric(12.0, 10.0)))
            .show(ctx, |ui| {
                if f.is_empty() {
                    return;
                }
                let cur = self.index.min(f.len() - 1);
                let thumb_w = 108.0;
                let avail = ui.available_width();
                let visible = ((avail / (thumb_w + 6.0)).floor() as usize).max(1);
                let half = visible / 2;
                let start = cur.saturating_sub(half);
                let end = (start + visible).min(f.len());
                ui.horizontal_centered(|ui| {
                    for (fi, &real) in f.iter().enumerate().take(end).skip(start) {
                        let (rect, resp) = ui.allocate_exact_size(Vec2::new(thumb_w, 72.0), Sense::click());
                        let (tex, tsize) = match self.thumbs.get(real) {
                            Some((t, _, s)) => (Some(t), s),
                            None => (None, Vec2::ZERO),
                        };
                        // 디코드가 3회 실패한 파일(decode_dead)은 썸네일이 영영 안 채워지므로
                        // 재요청·재페인트를 걸지 않는다(#64 유휴 보장). 실패 배지는 아래 ThumbInfo.failed로 그린다.
                        if tex.is_none() && !self.decode_dead(real) {
                            self.request_thumb(real, true); // 보이는 셀 우선
                            ui.ctx().request_repaint(); // 회색이면 채워질 때까지 재요청·재그리기(자가복구)
                        }
                        let it = &self.items[real];
                        let info = ThumbInfo {
                            label: it.entry.label,
                            raw_badge: it.entry.shows_raw_badge(),
                            raw_only: it.entry.has_raw && !it.entry.has_image,
                            active: fi == cur,
                            focused: false,
                            selected: false,
                            stars: it.entry.stars,
                            tag: it.entry.tag,
                            failed: self.decode_dead(real),
                        };
                        draw_thumb(ui, rect, tex, tsize, &info, self.badge_scale());
                        if resp.clicked() {
                            self.index = fi;
                        }
                    }
                });
            });
    }

    pub(super) fn ui_single(&mut self, ui: &mut egui::Ui) {
        let rect = ui.max_rect();
        let real = match self.current_real() {
            Some(r) => r,
            None => {
                self.ui_empty_state(ui);
                return;
            }
        };
        self.photo_view(ui, rect, real);
        if !self.has_modal() {
            let suffix = if self.full_raw { "ORIG · sRGB" } else { "FIT · sRGB" };
            self.paint_hud(ui, rect, real, suffix);
            self.ui_map_overlay(ui, rect, real);
        }
    }

    /// 빈 화면 사유 구분(#67): "폴더가 원래 비어 있음" vs "필터가 전량 배제함" vs (방어) 그 외.
    /// 단일뷰(ui_single)·그리드(ui_grid)가 공유 — 문구·버튼이 두 화면에서 따로 놀지 않게 한다.
    pub(super) fn ui_empty_state(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        let avail_h = ui.available_height();
        ui.vertical_centered(|ui| {
            ui.add_space((avail_h * 0.5 - 40.0).max(0.0));
            if self.items.is_empty() {
                ui.label(egui::RichText::new(tr(lang, "이 폴더에 표시할 사진이 없습니다")).color(theme::INK3));
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new("JPG · HEIC · PNG · RW2 · CR3 · ARW · NEF · DNG · …")
                        .font(mono(10.0))
                        .color(theme::INK4),
                );
            } else if self.any_filter_active() {
                ui.label(egui::RichText::new(tr(lang, "필터와 일치하는 사진이 없습니다")).color(theme::INK3));
                ui.add_space(10.0);
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new(format!("  {}  ", tr(lang, "필터 초기화")))
                                .color(Color32::from_rgb(0x0a, 0x14, 0x20)),
                        )
                        .fill(theme::ACCENT),
                    )
                    .clicked()
                {
                    self.reset_filters();
                }
            } else {
                // 방어 폴백: items도 있고 필터도 전부 기본값인데 목록이 비는 경우는 이론상 없어야
                // 하지만(filtered()가 AND 결합이라 논리적으로 도달 불가) 안전하게 옛 문구를 남긴다.
                ui.label(egui::RichText::new(tr(lang, "표시할 항목이 없습니다")).color(theme::INK3));
            }
        });
    }

    /// 사진 영역: 줌/이동 인터랙션 + 그리기.
    /// - 클릭(드래그 아님): 창맞춤(fit) ↔ 1:1 토글
    /// - Ctrl+휠 / 터치패드 핀치: 연속 확대·축소(커서 기준)
    /// - 드래그: 확대 상태에서 이동(pan)
    ///
    /// 프리뷰가 있으면 그것을, 없으면 썸네일을 열화 표시, 둘 다 없으면 "디코딩 중".
    pub(super) fn photo_view(&mut self, ui: &mut egui::Ui, area: Rect, real: usize) {
        let lang = self.lang;
        // 표시할 항목이 바뀌면: 확대 중이었으면 그 배율을 새 사진에 이어받고(#85),
        // 창맞춤이었으면 그대로 창맞춤.
        if self.zoom_for != Some(real) {
            self.zoom_for = Some(real);
            self.last_view_size = None; // 항목이 바뀜 → 해상도 추적 리셋(#48)
            self.af_zoom_pending = false;
            match self.keep_zoom {
                Some(_) => {
                    self.fit = false;
                    self.zoom_restore = true; // 텍스처 크기를 안 뒤에 아래에서 복원.
                }
                None => {
                    self.fit = true;
                    self.pan = Vec2::ZERO;
                }
            }
        }
        let texsize = self
            .cache
            .get(real)
            .map(|(t, _, s)| (t, s))
            .or_else(|| self.thumbs.get(real).map(|(t, _, s)| (t, s)));
        let (tex, size) = match texsize {
            Some(v) => v,
            None => {
                if self.decode_dead(real) {
                    // 누적 실패(#64): "디코딩 중…" 무한 반복 대신 에러 상태(⚠)로 고정 표시하고,
                    // 더 시도할 게 없으므로 재페인트 루프도 걸지 않는다.
                    // #75: 영역을 클릭하면 수동 재시도(NAS/네트워크 복구 후 폴더 재열기 없이 회복).
                    let name = self.items[real].entry.display.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                    let p = ui.painter();
                    p.text(area.center() + Vec2::new(0.0, -28.0), Align2::CENTER_CENTER, "⚠", mono(28.0), theme::WARN);
                    p.text(area.center() + Vec2::new(0.0, 0.0), Align2::CENTER_CENTER, tr(lang, "이 파일을 열 수 없습니다"), mono(12.0), theme::INK3);
                    p.text(area.center() + Vec2::new(0.0, 18.0), Align2::CENTER_CENTER, &name, mono(10.5), theme::INK4);
                    p.text(area.center() + Vec2::new(0.0, 40.0), Align2::CENTER_CENTER, tr(lang, "클릭하여 재시도"), mono(10.5), theme::INK3);
                    let resp = ui.interact(area, ui.id().with(("retry_dead", real)), Sense::click());
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if resp.clicked() {
                        self.retry_decode(real);
                        ui.ctx().request_repaint();
                    }
                } else {
                    ui.painter()
                        .text(area.center(), Align2::CENTER_CENTER, tr(lang, "디코딩 중…"), mono(12.0), theme::INK3);
                    ui.ctx().request_repaint();
                }
                return;
            }
        };
        if size.x <= 0.0 || size.y <= 0.0 {
            return;
        }
        let resp = ui.interact(area, ui.id().with("photoview"), Sense::click_and_drag());

        // 창맞춤 배율.
        let avail = Vec2::new((area.width() - 32.0).max(1.0), (area.height() - 32.0).max(1.0));
        let fit_scale = (avail.x / size.x).min(avail.y / size.y);
        if self.fit {
            self.zoom = fit_scale;
            self.pan = Vec2::ZERO;
        }
        let min_zoom = fit_scale.min(1.0);
        let max_zoom = 8.0_f32.max(fit_scale); // 최소 8x(작은 이미지도 확대 가능)

        // #85: 넘어온 확대 상태를 이 사진에 복원한다. `mag`는 긴 변이 차지하는 화면 px이라
        // 해상도·방향에 불변이다 — 가로(1920x1280) → 세로(1280x1920)로 넘어가도 긴 변은 그대로
        // 1920이라 픽셀 확인 배율이 똑같이 유지된다. pan은 표시 크기 대비 비율로 되돌린다.
        // 새 사진의 창맞춤 배율이 더 크면(작은 이미지) 조용히 창맞춤으로 내려앉는다.
        let restored = self.zoom_restore;
        if restored {
            if let Some((mag, pan_norm)) = self.keep_zoom {
                let z = (mag / size.max_elem().max(1.0)).clamp(min_zoom, max_zoom);
                if z <= fit_scale * 1.001 {
                    self.fit = true;
                    self.pan = Vec2::ZERO;
                } else {
                    self.zoom = z;
                    self.pan = Vec2::new(pan_norm.x * size.x * z, pan_norm.y * size.y * z);
                }
            }
            // 프리뷰(정식 해상도)가 도착했으면 복원 확정. 그 전까지는 썸네일 크기에 맞춘
            // 임시 값이라 매 프레임 다시 맞춘다.
            if self.fit || self.cache.contains(real) {
                self.zoom_restore = false;
            }
        }

        // #48: 같은 항목에서 표시 해상도가 바뀌면(ORIG 로드/언로드) 화면상 배율·보던 위치를 유지한다.
        // zoom은 화면픽셀/이미지픽셀이라 해상도가 K배면 같은 zoom이 K배 확대로 보인다 → zoom을 1/K로
        // 맞춰 scaled(=size*zoom, 화면상 크기)를 보존하면 pan(화면픽셀)도 그대로 들어맞는다.
        // 긴 변 기준으로 비교해야 가로↔세로가 바뀌어도(같은 항목이라도 캐시된 옛 방향 →
        // 새로 회전된 프리뷰) 종횡비만큼 배율이 튀지 않는다.
        // 복원 프레임에서는 zoom을 절대값으로 방금 정했으므로 건너뛴다.
        if !self.fit && !restored {
            if let Some(prev) = self.last_view_size {
                let (pl, sl) = (prev.max_elem(), size.max_elem());
                if pl > 0.0 && sl > 0.0 && (pl - sl).abs() > 0.5 {
                    self.zoom = (self.zoom * pl / sl).clamp(min_zoom, max_zoom);
                }
            }
        }
        self.last_view_size = Some(size);

        // 줌 입력: Ctrl+휠 또는 터치패드 핀치(커서 기준 확대).
        if resp.hovered() {
            let (scroll_y, zd, mods, ptr) = ui.input(|i| {
                (
                    i.raw_scroll_delta.y,
                    i.zoom_delta(),
                    i.modifiers,
                    i.pointer.hover_pos(),
                )
            });
            let mut nz = self.zoom;
            if (zd - 1.0).abs() > 1e-4 {
                nz *= zd; // 핀치(또는 egui가 접어 넣은 Ctrl+휠)
            } else if (mods.ctrl || mods.command) && scroll_y.abs() > 0.0 {
                nz *= (scroll_y * 0.004).exp(); // Ctrl+휠 → 매끄러운 곱셈 줌
            }
            nz = nz.clamp(min_zoom, max_zoom);
            if (nz - self.zoom).abs() > f32::EPSILON {
                if let Some(p) = ptr {
                    // 커서 아래 지점이 고정되도록 pan 보정.
                    let a = p - area.center();
                    let k = nz / self.zoom;
                    self.pan = a * (1.0 - k) + self.pan * k;
                }
                self.zoom = nz;
                self.fit = nz <= fit_scale * 1.001;
                self.zoom_restore = false; // 사용자가 직접 조작 → 복원 대기 취소(#85)
            }
        }

        // 클릭(드래그 아님): fit ↔ 1:1.
        if resp.clicked() {
            if self.fit {
                self.fit = false;
                self.zoom = 1.0;
                self.pan = Vec2::ZERO;
            } else {
                self.fit = true;
            }
            self.zoom_restore = false;
        }
        // 드래그: 이동.
        if resp.dragged() {
            self.pan += resp.drag_delta();
            self.zoom_restore = false;
        }

        // pan 클램프(이미지가 영역보다 클 때만 이동 허용 — 화면 밖으로 날아가지 않게).
        let scaled = size * self.zoom;
        // #49: AF 중심 확대 요청 소비 — 측거점(합초 우선)이 화면 중앙에 오도록 pan을 설정한다.
        // 표시좌표(0..1)의 (cx,cy)가 area.center()에 오려면 pan = scaled * (0.5 - c). 이후 클램프로
        // 가장자리 측거점이어도 화면 밖으로 벗어나지 않게 보정된다.
        if self.af_zoom_pending {
            self.af_zoom_pending = false;
            if let Some((cx, cy)) = self
                .items
                .get(real)
                .and_then(|it| it.af.as_ref().map(|af| (af, it.orient.unwrap_or(1))))
                .and_then(|(af, orient)| af_focus_center(af, orient))
            {
                self.pan = Vec2::new(scaled.x * (0.5 - cx as f32), scaled.y * (0.5 - cy as f32));
            }
        }
        let max_px = ((scaled.x - area.width()) * 0.5).max(0.0);
        let max_py = ((scaled.y - area.height()) * 0.5).max(0.0);
        self.pan.x = self.pan.x.clamp(-max_px, max_px);
        self.pan.y = self.pan.y.clamp(-max_py, max_py);

        // #85: 다음 사진으로 넘어가도 이어받을 확대 상태를 기록한다. 창맞춤이면 None(넘겨도 창맞춤).
        // 복원이 아직 확정되지 않은 프레임(썸네일 임시 표시)에서는 덮어쓰지 않는다 — 320px에
        // 맞춰 클램프된 값이 원래 배율을 영구히 깎아버리기 때문.
        if !self.zoom_restore {
            self.keep_zoom = (!self.fit).then(|| {
                (
                    size.max_elem() * self.zoom,
                    Vec2::new(self.pan.x / scaled.x.max(1.0), self.pan.y / scaled.y.max(1.0)),
                )
            });
        }

        // 그리기(영역으로 클립).
        let uv = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0));
        let target = Rect::from_center_size(area.center() + self.pan, scaled);
        ui.painter().with_clip_rect(area).image(tex, target, uv, Color32::WHITE);

        // AF 포인트 오버레이(#37): 측거점 사각형을 사진 위에 그린다. 좌표는 센서(미회전)
        // 기준 0..1 정규화이므로 EXIF orientation으로 표시 좌표계로 돌린다. 데이터 없는
        // 바디·MF는 af가 None → 조용히 미표시.
        if self.show_af {
            // AF·orientation은 백그라운드 메타 로더(request_meta)가 채운다 — 도착 전엔 미표시.
            if let Some(it) = self.items.get(real) {
                if let Some(af) = &it.af {
                    let orient = it.orient.unwrap_or(1);
                    let p = ui.painter().with_clip_rect(area);
                    for pt in &af.points {
                        let (cx, cy, w, h) = af_display_coords(pt, orient);
                        let center = Pos2::new(
                            target.min.x + cx as f32 * target.width(),
                            target.min.y + cy as f32 * target.height(),
                        );
                        // 크기 미기록(0.0)은 고정 비율 박스 폴백(구형 캐논 단일점·소니 위치점).
                        let bw = if w > 0.0 { w as f32 * target.width() } else { 0.045 * target.width().min(target.height()) };
                        let bh = if h > 0.0 { h as f32 * target.height() } else { bw };
                        let r = Rect::from_center_size(center, Vec2::new(bw.max(8.0), bh.max(8.0)));
                        // 합초점은 초록 굵게, 선택만 된 점은 밝게, 나머지는 흐리게(DPP 스타일).
                        // `width`는 `Stroke::new(impl Into<f32>)`로 흘러가므로 접미사로 f32를
                        // 못박는다(#84: 접미사 없는 리터럴의 f32 폴백은 향후 hard error).
                        let (color, width) = if pt.in_focus {
                            (theme::OK, 2.0_f32)
                        } else if pt.selected {
                            (theme::INK2, 1.2_f32)
                        } else {
                            (theme::INK4, 1.0_f32)
                        };
                        p.rect_stroke(r, Rounding::same(2.0), Stroke::new(width, color));
                    }
                }
            }
        }

        // 이동 가능하면 손 커서.
        if !self.fit && (max_px > 0.0 || max_py > 0.0) {
            let cur = if resp.dragged() {
                egui::CursorIcon::Grabbing
            } else {
                egui::CursorIcon::Grab
            };
            ui.ctx().set_cursor_icon(cur);
        }
    }

    pub(super) fn paint_hud(&self, ui: &egui::Ui, area: Rect, real: usize, counter_suffix: &str) {
        let lang = self.lang;
        let it = &self.items[real];
        let f = self.filtered();
        // TL: 라벨 + 파일명.
        let name = it.entry.display.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let mut tl = area.left_top() + Vec2::new(20.0, 22.0);
        let chip = format!("[{}]", it.entry.label.name(lang));
        hud_text(ui, tl, Align2::LEFT_TOP, &chip, mono(12.0), theme::label_color(it.entry.label));
        tl.x += 64.0;
        let badge = if it.entry.shows_raw_badge() { "  +RAW" } else { "" };
        hud_text(ui, tl, Align2::LEFT_TOP, &format!("{name}{badge}"), mono(12.0), theme::INK);
        // 별점(#23): 라벨/파일명 아래 줄에 채워진 별로 표시.
        if it.entry.stars > 0 {
            let stars_str = "★".repeat(it.entry.stars.min(5) as usize);
            hud_text(ui, area.left_top() + Vec2::new(20.0, 42.0), Align2::LEFT_TOP, &stars_str, mono(13.0), theme::HOLD);
        }

        // TR: 카운터.
        let tr = area.right_top() + Vec2::new(-18.0, 14.0);
        hud_text(ui, tr, Align2::RIGHT_TOP, &format!("{:03} / {}", (self.index + 1).min(f.len().max(1)), f.len()), mono(30.0), theme::INK);
        hud_text(ui, tr + Vec2::new(0.0, 34.0), Align2::RIGHT_TOP, counter_suffix, mono(10.0), theme::INK3);

        // BL: EXIF.
        if self.show_exif {
            if let Some(ex) = &it.exif {
                let mut y = area.bottom() - 16.0;
                let lines = exif_lines(ex);
                for line in lines.iter().rev() {
                    hud_text(ui, Pos2::new(area.left() + 16.0, y), Align2::LEFT_BOTTOM, line, mono(12.0), theme::INK2);
                    y -= 18.0;
                }
            }
        }

        // BR: 히스토그램.
        if self.show_hist {
            if let Some(h) = self.histo.get(&real) {
                let w = 232.0_f32.min(area.width() * 0.35);
                let hh = 56.0;
                let hr = Rect::from_min_size(
                    Pos2::new(area.right() - w - 16.0, area.bottom() - hh - 16.0),
                    Vec2::new(w, hh),
                );
                widgets::draw_histogram(ui, hr, &h.bins, h.max);
            }
        }

    }

    pub(super) fn ui_grid(&mut self, ui: &mut egui::Ui) {
        let f = self.filtered();
        if f.is_empty() {
            // 예전엔 문구 없이 빈 캔버스였다(#67) — 단일뷰와 같은 헬퍼로 사유를 보여준다.
            self.ui_empty_state(ui);
            return;
        }
        let cols = self.grid_cols.clamp(4, 12);
        let cur = self.index.min(f.len().saturating_sub(1));
        let gap = 6.0;
        let avail = ui.available_width();
        let cell_w = ((avail - gap * (cols as f32 - 1.0)) / cols as f32).max(40.0);
        let cell_h = (cell_w * 0.7).clamp(80.0, 160.0);
        let row_h = cell_h + gap;
        let rows = f.len().div_ceil(cols);
        let avail_h = ui.available_height();
        // 키보드 내비로 예약된 스크롤: 대상 행이 화면 밖일 때만 가장자리 정렬로 따라간다
        // (화면 안 작은 이동에서는 튀지 않게).
        let mut area = egui::ScrollArea::vertical();
        if let Some(target_row) = self.grid_scroll_to.take() {
            let vis = self.grid_visible_rows.clone();
            if vis.is_empty() || target_row < vis.start || target_row >= vis.end {
                let off = if target_row < vis.start {
                    target_row as f32 * row_h // 위로: 상단 정렬
                } else {
                    ((target_row + 1) as f32 * row_h - avail_h).max(0.0) // 아래로: 하단 정렬
                };
                let max_off = (rows as f32 * row_h - avail_h).max(0.0);
                area = area.vertical_scroll_offset(off.clamp(0.0, max_off));
            }
        }
        // 가상화: 보이는 행만 생성·디코딩(수천 장에서도 매 프레임 비용이 일정).
        area.show_rows(ui, row_h, rows, |ui, row_range| {
            self.grid_visible_rows = row_range.clone();
            ui.spacing_mut().item_spacing = Vec2::new(gap, gap);
            // 가시 범위 위아래 여유분(±2행)까지 미리 우선 요청 → 스크롤 가장자리·부분행 회색 방지.
            let pf_start = row_range.start.saturating_sub(2);
            let pf_end = (row_range.end + 2).min(rows);
            for row in pf_start..pf_end {
                for c in 0..cols {
                    let fi = row * cols + c;
                    if fi < f.len() && !self.thumbs.contains(f[fi]) && !self.decode_dead(f[fi]) {
                        self.request_thumb(f[fi], true);
                    }
                }
            }
            for row in row_range {
                ui.horizontal(|ui| {
                    for c in 0..cols {
                        let fi = row * cols + c;
                        if fi >= f.len() {
                            break;
                        }
                        let real = f[fi];
                        let (rect, resp) =
                            ui.allocate_exact_size(Vec2::new(cell_w, cell_h), Sense::click());
                        let (tex, tsize) = match self.thumbs.get(real) {
                            Some((t, _, s)) => (Some(t), s),
                            None => (None, Vec2::ZERO),
                        };
                        // 디코드가 3회 실패한 파일(decode_dead)은 썸네일이 영영 안 채워지므로
                        // 재요청·재페인트를 걸지 않는다(#64 유휴 보장). 실패 배지는 아래 ThumbInfo.failed로 그린다.
                        if tex.is_none() && !self.decode_dead(real) {
                            self.request_thumb(real, true); // 보이는 셀 우선
                            ui.ctx().request_repaint(); // 회색이면 채워질 때까지 재요청·재그리기(자가복구)
                        }
                        let it = &self.items[real];
                        let info = ThumbInfo {
                            label: it.entry.label,
                            raw_badge: it.entry.shows_raw_badge(),
                            raw_only: it.entry.has_raw && !it.entry.has_image,
                            active: fi == cur,
                            focused: false,
                            selected: self.selected.contains(&real),
                            stars: it.entry.stars,
                            tag: it.entry.tag,
                            failed: self.decode_dead(real),
                        };
                        draw_thumb(ui, rect, tex, tsize, &info, self.badge_scale());
                        if resp.clicked() {
                            // Ctrl/⌘+클릭 = 토글, Shift+클릭 = 앵커~클릭 범위, 그냥 클릭 = 단일.
                            let m = ui.input(|i| i.modifiers);
                            if m.command {
                                if !self.selected.insert(real) {
                                    self.selected.remove(&real);
                                }
                                self.sel_anchor = Some(fi);
                            } else if m.shift {
                                let a = self.sel_anchor.unwrap_or(fi);
                                let (lo, hi) = (a.min(fi), a.max(fi));
                                for k in lo..=hi {
                                    if k < f.len() {
                                        self.selected.insert(f[k]);
                                    }
                                }
                            } else {
                                self.selected.clear();
                                self.sel_anchor = Some(fi);
                            }
                            self.index = fi;
                        }
                        if resp.double_clicked() {
                            self.index = fi;
                            self.view = ViewMode::Single;
                        }
                    }
                });
            }
        });
    }

    /// GPS 미니 지도 패널(#38). 우상단 코너, 현재 항목에 GPS가 있을 때만. 합성은
    /// 백그라운드 스레드(타일: 디스크 캐시 → OSM), 시작은 사진 디코딩이 유휴일 때만.
    /// 클릭하면 브라우저에서 OSM으로 크게 보기, ±로 줌(3~17).
    pub(super) fn ui_map_overlay(&mut self, ui: &mut egui::Ui, area: Rect, real: usize) {
        if !self.show_map {
            self.map_state = None;
            return;
        }
        let lang = self.lang;
        // EXIF는 백그라운드 메타 로더(request_meta)가 채운다 — GPS가 도착하면 그 프레임부터 표시.
        let Some(gps) = self.items.get(real).and_then(|it| it.exif.as_ref()).and_then(|e| e.gps) else {
            self.map_state = None;
            return; // GPS 없는 사진은 자동 미표시.
        };
        const MW: u32 = 220;
        const MH: u32 = 150;
        let zoom = self.map_zoom;
        // 항목·줌이 바뀌면 새로 합성. 단, 시작은 디코딩 유휴 시에만(사진 표시가 우선, #33 패턴).
        let stale = self.map_state.as_ref().map(|m| m.real != real || m.zoom != zoom).unwrap_or(true);
        if stale {
            let idle = self.pending_preview.is_empty() && self.pending_thumb.is_empty();
            if idle {
                let (tx, rx) = crossbeam_channel::bounded(1);
                let cache = crate::map::cache_dir();
                let (lat, lon) = (gps.lat, gps.lon);
                std::thread::spawn(move || {
                    let _ = tx.send(crate::map::compose(lat, lon, zoom, MW, MH, &cache));
                });
                self.map_state = Some(MapState {
                    real,
                    zoom,
                    lat: gps.lat,
                    lon: gps.lon,
                    rx: Some(rx),
                    tex: None,
                    failed: false,
                });
            } else {
                // 디코딩 끝나면 시작하도록 다음 프레임 재확인.
                ui.ctx().request_repaint_after(Duration::from_millis(200));
                if self.map_state.is_none() {
                    return;
                }
            }
        }
        // 결과 수신 → 텍스처 업로드(네트워크 스레드는 egui를 못 깨우므로 폴링).
        if let Some(st) = &mut self.map_state {
            if let Some(rx) = &st.rx {
                match rx.try_recv() {
                    Ok(Some(img)) => {
                        let ci = egui::ColorImage::from_rgba_unmultiplied([img.w as usize, img.h as usize], &img.rgba);
                        st.tex = Some(ui.ctx().load_texture(
                            format!("map_{}_{}", st.real, st.zoom),
                            ci,
                            egui::TextureOptions::LINEAR,
                        ));
                        st.rx = None;
                    }
                    Ok(None) => {
                        st.failed = true; // 오프라인 + 캐시 없음.
                        st.rx = None;
                    }
                    Err(crossbeam_channel::TryRecvError::Disconnected) => {
                        st.failed = true;
                        st.rx = None;
                    }
                    Err(crossbeam_channel::TryRecvError::Empty) => {
                        ui.ctx().request_repaint_after(Duration::from_millis(300));
                    }
                }
            }
        }
        let Some(st) = &self.map_state else { return };
        let (tex_id, failed, lat, lon) = (st.tex.as_ref().map(|t| t.id()), st.failed, st.lat, st.lon);

        // 패널: 우상단 카운터 아래.
        let panel = Rect::from_min_size(
            Pos2::new(area.right() - MW as f32 - 18.0, area.top() + 64.0),
            Vec2::new(MW as f32, MH as f32),
        );
        let p = ui.painter();
        p.rect_filled(panel.expand(4.0), Rounding::same(6.0), Color32::from_black_alpha(150));
        if let Some(tid) = tex_id {
            p.image(tid, panel, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            // 위치 마커(패널 정중앙 = 촬영 좌표).
            let c = panel.center();
            p.circle_filled(c, 5.0, theme::REJECT);
            p.circle_stroke(c, 5.0, Stroke::new(1.5_f32, Color32::WHITE));
            // OSM attribution(타일 정책 필수). 지도 위 가독성용 어두운 띠.
            let att = panel.with_min_y(panel.bottom() - 13.0);
            p.rect_filled(att, Rounding::ZERO, Color32::from_black_alpha(120));
            p.text(
                att.right_center() + Vec2::new(-4.0, 0.0),
                Align2::RIGHT_CENTER,
                "© OpenStreetMap contributors",
                mono(8.0),
                Color32::from_gray(0xdd),
            );
        } else {
            let msg = if failed { tr(lang, "지도를 불러올 수 없음 (오프라인?)") } else { tr(lang, "지도 로딩…") };
            p.text(panel.center(), Align2::CENTER_CENTER, msg, mono(10.0), theme::INK3);
        }
        // 좌표 라벨(패널 아래).
        p.text(
            panel.left_bottom() + Vec2::new(0.0, 8.0),
            Align2::LEFT_TOP,
            format!("{:.5}, {:.5}", lat, lon),
            mono(9.0),
            theme::INK3,
        );
        // 클릭 → 브라우저로 크게 보기.
        let resp = ui.interact(panel, ui.id().with("map_panel"), Sense::click());
        if resp.clicked() {
            open_url(&crate::map::osm_url(lat, lon, zoom.max(15)));
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        // 줌 ±(패널 좌상단).
        for (i, (label, delta)) in [("+", 1i16), ("−", -1i16)].iter().enumerate() {
            let br = Rect::from_min_size(panel.left_top() + Vec2::new(4.0 + i as f32 * 22.0, 4.0), Vec2::splat(18.0));
            let r = ui.interact(br, ui.id().with(("map_zoom_btn", i)), Sense::click());
            let pb = ui.painter();
            pb.rect_filled(br, Rounding::same(4.0), Color32::from_black_alpha(170));
            pb.text(br.center(), Align2::CENTER_CENTER, *label, mono(12.0), if r.hovered() { theme::INK } else { theme::INK2 });
            if r.clicked() {
                self.map_zoom = (self.map_zoom as i16 + delta).clamp(3, 17) as u8;
            }
        }
    }

    pub(super) fn ui_fullscreen(&mut self, ctx: &egui::Context) {
        let bg = self.photo_bg();
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(bg))
            .show(ctx, |ui| {
                let rect = ui.max_rect();
                if let Some(real) = self.current_real() {
                    self.photo_view(ui, rect, real);
                    self.paint_hud(ui, rect, real, "FULLSCREEN · ESC");
                    self.ui_map_overlay(ui, rect, real);
                }
            });
    }

    /// 우하단 토스트 오버레이(#61). 심각도 점 + 메시지. 상태바를 가리지 않게 STATUS_H+여백
    /// 만큼 띄우고, 모든 모달·배너 위(Foreground)에 그린다. Area/Frame 구성은 전송·컬링
    /// 카드(ui_ai_cull_dialog 등)와 같은 패턴을 그대로 답습해 egui 0.29 API 사용을 맞춘다.
    ///
    /// #76: **오류 토스트만** 클릭-투-디스미스(자동 만료가 없어 이 클릭·✕가 유일한 닫기 수단)로
    /// 상호작용시키고, 정보(3초)·공지(6초)는 `Area::interactable(false)`로 포인터를 통과시킨다.
    /// 예전엔 모든 토스트가 Foreground Area + Sense::click()이라, 자동소멸 토스트가 떠 있는
    /// 동안 그 사각형에 겹치는 우하단 위젯(그리드 썸네일 등) 클릭을 삼켰다(닫히기만 하고 선택 안 됨).
    /// interactable(false)면 그 레이어가 히트테스트 대상에서 빠져 하단 위젯이 클릭을 정상 수신한다.
    pub(super) fn ui_toast(&mut self, ctx: &egui::Context) {
        let Some(t) = &self.toast else { return };
        // 심각도별 상태 점 색(기존 테마 상수 재사용): 오류=REJECT, 알림=ACCENT, 정보=INK3.
        let dot = match t.kind {
            ToastKind::Error => theme::REJECT,
            ToastKind::Notice => theme::ACCENT,
            ToastKind::Info => theme::INK3,
        };
        // 오류만 닫기(✕)를 함께 보이고, 오직 오류만 클릭을 받는다(정보/알림은 자동 소멸 + 클릭 통과).
        let is_error = t.kind == ToastKind::Error;
        let text = t.text.clone();
        let resp = egui::Area::new(egui::Id::new("toast_overlay"))
            .order(egui::Order::Foreground)
            .interactable(is_error)
            .anchor(Align2::RIGHT_BOTTOM, Vec2::new(-16.0, -(STATUS_H + 12.0)))
            .show(ctx, |ui| {
                let inner = egui::Frame::none()
                    .fill(theme::BG3)
                    .stroke(Stroke::new(1.0_f32, theme::LINE2))
                    .rounding(6.0)
                    .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                    .show(ui, |ui| {
                        ui.set_max_width(380.0);
                        // 메시지는 여러 줄일 수 있어(예: sha256 불일치는 \n 포함) 남는 폭에서
                        // 줄바꿈되도록 horizontal_wrapped를 쓴다(전송 다이얼로그와 같은 패턴).
                        ui.horizontal_wrapped(|ui| {
                            let (r, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
                            ui.painter().circle_filled(r.center(), 4.0, dot);
                            ui.add_space(4.0);
                            ui.label(egui::RichText::new(&text).font(prop(12.0)).color(theme::INK));
                            if is_error {
                                ui.add_space(4.0);
                                ui.label(egui::RichText::new("✕").font(prop(12.0)).color(theme::INK3));
                            }
                        })
                    })
                    .response;
                // 오류만 클릭 상호작용을 붙인다(정보/알림은 interactable(false)라 어차피 클릭을 안 받음).
                if is_error {
                    inner.interact(Sense::click())
                } else {
                    inner
                }
            })
            .inner;
        if is_error {
            if resp.hovered() {
                ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.clicked() {
                self.toast = None;
            }
        }
    }
}
