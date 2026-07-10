//! 소형 모달(분해 3/8): 점프(#52)·일괄 분류 변경(#3) 다이얼로그.
//! app.rs에서 순수 이동 — 동작 변경 없음.

use super::*;

impl RawBlowApp {
    pub(super) fn ui_jump(&mut self, ctx: &egui::Context) {
        let lang = self.lang;
        let mut close = false;
        let mut go = false;
        let total = self.filtered().len();
        egui::Window::new("jump_dialog")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_TOP, Vec2::new(0.0, 80.0))
            .fixed_size(Vec2::new(420.0, 0.0))
            .frame(modal_frame())
            .show(ctx, |ui| {
                modal_header(ui, tr(lang, "점프"), tr(lang, "한 곳으로 이동 — 순번 또는 파일명 일부"));
                // 모드 선택(#52): 순번(기본) / 파일명. 하나만 활성.
                let sel = if self.jump_by_number { 0 } else { 1 };
                if let Some(i) = segmented(
                    ui,
                    &[
                        (tr(lang, "순번"), tr(lang, "현재/전체의 번호")),
                        (tr(lang, "파일명"), tr(lang, "일부 일치")),
                    ],
                    sel,
                ) {
                    self.jump_by_number = i == 0;
                }
                ui.add_space(8.0);
                let hint = if self.jump_by_number {
                    trf(lang, "1 ~ {} 사이 번호", &[&total.max(1).to_string()])
                } else {
                    tr(lang, "파일명 일부(한 개)").to_string()
                };
                // 단일 값 입력(#52): singleline이라 줄바꿈 불가. 숫자 모드면 숫자만 통과.
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut self.jump_text)
                        .font(mono(12.0))
                        .desired_width(f32::INFINITY)
                        .hint_text(hint),
                );
                if self.jump_by_number {
                    // 숫자 모드: 쉼표·문자 등은 입력 즉시 제거(여러 값·구분자 차단).
                    self.jump_text.retain(|c| c.is_ascii_digit());
                } else {
                    // 파일명 모드: 줄바꿈·쉼표·탭 제거(단일 값 강제).
                    self.jump_text.retain(|c| c != ',' && c != '\t' && c != '\n' && c != '\r');
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(egui::Button::new(egui::RichText::new(format!("  {}  ⏎  ", tr(lang, "점프"))).color(Color32::from_rgb(0x0a, 0x14, 0x20))).fill(theme::ACCENT)).clicked() {
                            go = true;
                        }
                        ui.add_space(8.0);
                        if toggle_btn(ui, tr(lang, "닫기 (Esc)"), false).clicked() {
                            close = true;
                        }
                    });
                });
                let _ = resp;
            });
        if go || ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            let q = self.jump_text.trim().to_string();
            if self.jump_by_number {
                // 순번 점프: 1-based 필터 위치(우상단 "현재 / 전체"와 동일).
                match q.parse::<usize>() {
                    Ok(n) if n >= 1 && n <= total => {
                        self.index = n - 1;
                        close = true;
                    }
                    _ => {
                        self.toast_info(trf(lang, "1 ~ {} 사이 번호를 입력하세요", &[&total.max(1).to_string()]));
                    }
                }
            } else if q.is_empty() {
                self.toast_info(tr(lang, "파일명 일부를 입력하세요").into());
            } else {
                // 파일명 점프: 단일 항(부분일치, 대소문자 무시) → 첫 매칭.
                let entries: Vec<Entry> = self.items.iter().map(|i| i.entry.clone()).collect();
                let hits = transfer::match_indices(&entries, &[q], MatchMode::Contains);
                if let Some(&first) = hits.first() {
                    let f = self.filtered();
                    if let Some(pos) = f.iter().position(|&r| r == first) {
                        self.index = pos;
                    }
                    self.toast_info(trf(lang, "{} 건 매칭 — 첫 항목으로", &[&hits.len().to_string()]));
                    close = true;
                } else {
                    self.toast_info(tr(lang, "매칭 없음").into());
                }
            }
        }
        if close {
            self.jump_text.clear();
        }
        if close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.jump_open = false;
        }
    }

    /// 그리드 모드 한정 일괄 분류 변경 모달(#3).
    ///
    /// 파일명(또는 일부)으로 검색해 매칭되는 항목을 한꺼번에 Q/W/E/R 라벨로 적용한다.
    /// 매칭 규칙은 RawPull과 동일(stem 기준 contains/exact, 대소문자 무시) —
    /// transfer::parse_terms / match_indices를 그대로 재사용한다.
    pub(super) fn ui_bulk(&mut self, ctx: &egui::Context) {
        let lang = self.lang;
        let mut close = false;
        let mut search = false;
        let mut apply = false;
        egui::Window::new("bulk_dialog")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_TOP, Vec2::new(0.0, 80.0))
            .fixed_size(Vec2::new(520.0, 0.0))
            .frame(modal_frame())
            .show(ctx, |ui| {
                modal_header(ui, tr(lang, "일괄 분류 변경"), tr(lang, "파일명·일부 → 매칭 → 라벨 적용"));
                ui.add(
                    egui::TextEdit::multiline(&mut self.bulk_text)
                        .font(mono(12.0))
                        .desired_rows(3)
                        .desired_width(f32::INFINITY)
                        .hint_text(tr(lang, "파일명 또는 일부 — 줄바꿈·쉼표·탭으로 구분")),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.bulk_exact, tr(lang, "정확히 일치"));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new(format!("  {}  ⏎  ", tr(lang, "검색")))
                                        .color(Color32::from_rgb(0x0a, 0x14, 0x20)),
                                )
                                .fill(theme::ACCENT),
                            )
                            .clicked()
                        {
                            search = true;
                        }
                    });
                });

                // 검색이 끝났으면 결과 + 라벨 선택 영역 노출.
                if self.bulk_searched {
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new(trf(lang, "매칭 {}건", &[&self.bulk_hits.len().to_string()]))
                            .font(prop(11.0))
                            .color(theme::INK2),
                    );
                    ui.add_space(4.0);
                    egui::ScrollArea::vertical()
                        .max_height(200.0)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            if self.bulk_hits.is_empty() {
                                ui.label(
                                    egui::RichText::new(tr(lang, "매칭 결과 없음"))
                                        .font(mono(11.0))
                                        .color(theme::INK3),
                                );
                            } else {
                                for &idx in &self.bulk_hits {
                                    if let Some(it) = self.items.get(idx) {
                                        let lbl = it.entry.label;
                                        let mark = match lbl {
                                            Label::Pick => "●",
                                            Label::Hold => "◐",
                                            Label::Reject => "✕",
                                            Label::Unrated => "·",
                                        };
                                        let [r, g, b] = lbl.color_rgb();
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                egui::RichText::new(mark)
                                                    .font(mono(11.0))
                                                    .color(Color32::from_rgb(r, g, b)),
                                            );
                                            ui.label(
                                                egui::RichText::new(&it.entry.stem)
                                                    .font(mono(11.0))
                                                    .color(theme::INK2),
                                            );
                                        });
                                    }
                                }
                            }
                        });
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new(tr(lang, "적용할 라벨"))
                            .font(prop(11.0))
                            .color(theme::INK3),
                    );
                    ui.horizontal(|ui| {
                        for (lbl, key) in [
                            (Label::Pick, "Q"),
                            (Label::Hold, "W"),
                            (Label::Reject, "E"),
                            (Label::Unrated, "R"),
                        ] {
                            let active = self.bulk_target == lbl;
                            if toggle_btn(ui, &format!("{}  {}", key, lbl.name(lang)), active).clicked() {
                                self.bulk_target = lbl;
                            }
                        }
                    });
                }

                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let can_apply = self.bulk_searched && !self.bulk_hits.is_empty();
                        let btn = egui::Button::new(
                            egui::RichText::new(format!("  {}  ", tr(lang, "적용")))
                                .color(Color32::from_rgb(0x0a, 0x14, 0x20)),
                        )
                        .fill(theme::ACCENT);
                        if ui.add_enabled(can_apply, btn).clicked() {
                            apply = true;
                        }
                        ui.add_space(8.0);
                        if toggle_btn(ui, tr(lang, "닫기 (Esc)"), false).clicked() {
                            close = true;
                        }
                    });
                });
            });

        // 검색 트리거(버튼 또는 Enter — 입력이 비어있지 않을 때만).
        if search
            || (ctx.input(|i| i.key_pressed(egui::Key::Enter))
                && !self.bulk_text.trim().is_empty())
        {
            let terms = transfer::parse_terms(&self.bulk_text);
            let entries: Vec<Entry> = self.items.iter().map(|i| i.entry.clone()).collect();
            let mode = if self.bulk_exact {
                MatchMode::Exact
            } else {
                MatchMode::Contains
            };
            self.bulk_hits = transfer::match_indices(&entries, &terms, mode);
            self.bulk_searched = true;
        }

        // 적용: 매칭된 모든 항목에 라벨을 일괄 적용하고 사이드카 저장을 앞당긴다.
        if apply && !self.bulk_hits.is_empty() {
            // 컬링이 라벨 축을 잠갔으면 일괄 라벨링도 막는다(결과 덮어쓰기 혼동 방지).
            if self.cull_axis_locked(AiCullTarget::Label) {
                self.bulk_open = false;
                return;
            }
            let target = self.bulk_target;
            let mut changed = 0usize;
            for &idx in &self.bulk_hits {
                if let Some(it) = self.items.get_mut(idx) {
                    if it.entry.label != target {
                        it.entry.label = target;
                        changed += 1;
                    }
                }
            }
            if changed > 0 {
                self.sidecar_dirty = true;
                // 다음 틱에서 즉시 사이드카가 저장되도록 last_save를 과거로.
                self.last_save = Instant::now() - Duration::from_millis(400);
            }
            self.toast_info(
                trf(lang, "{}건 → {}", &[&self.bulk_hits.len().to_string(), target.name(lang)]),
            );
            self.bulk_open = false;
            return;
        }

        if close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.bulk_open = false;
        }
    }
}
