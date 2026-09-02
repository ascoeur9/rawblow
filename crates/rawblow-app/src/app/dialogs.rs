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
                        self.keep_view_mode_on_move(); // 화살표 이동과 같은 규칙(#85/#87)
                        close = true;
                    }
                    _ => {
                        self.toast_info(trf(lang, "1 ~ {} 사이 번호를 입력하세요", &[&total.max(1).to_string()]));
                    }
                }
            } else if q.is_empty() {
                self.toast_info(tr(lang, "파일명 일부를 입력하세요").into());
            } else {
                // 파일명 점프: 현재 필터 안에서만 찾는다(#99). 필터 밖 히트는 이동하지 않는다.
                let f = self.filtered();
                let entries: Vec<Entry> = f.iter().map(|&i| self.items[i].entry.clone()).collect();
                let hits = transfer::match_indices(&entries, &[q], MatchMode::Contains);
                if let Some(&local) = hits.first() {
                    self.index = local;
                    self.keep_view_mode_on_move();
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
            let hits = self.bulk_hits.clone();
            let mut changed = 0usize;
            self.push_undo(&hits);
            for &idx in &hits {
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
                trf(lang, "{}건 → {}", &[&changed.to_string(), target.name(lang)]),
            );
            self.bulk_open = false;
            return;
        }

        if close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.bulk_open = false;
        }
    }

    /// 단축키 치트시트 오버레이(#66). 키보드 중심 앱인데 단축키 안내가 앱 안에 없어
    /// (M/A/Z/F/백틱/⇧0 등은 어디에도 미표시) 소스나 README를 봐야 했다 — ?/F1로 여닫는다.
    /// 표시 내용은 handle_keys의 키맵과 1:1로 맞춘다. has_modal이 열린 동안 전역 키를 막으므로
    /// (점프·일괄과 동일) Esc·?·F1·배경 클릭을 오버레이가 직접 받아 닫는다.
    pub(super) fn ui_help(&mut self, ctx: &egui::Context) {
        let lang = self.lang;
        let mut close = false;

        // 한 줄 = 키캡 배지(들) + 설명. 여러 키는 배지를 나란히 둔다.
        fn row(ui: &mut egui::Ui, keys: &[&str], desc: &str) {
            ui.horizontal(|ui| {
                for k in keys {
                    kbd(ui, k);
                    ui.add_space(3.0);
                }
                ui.add_space(3.0);
                ui.label(egui::RichText::new(desc).font(prop(12.0)).color(theme::INK2));
            });
            ui.add_space(6.0);
        }
        // 섹션 머리(대문자 작은 라벨 — section_head와 같은 톤).
        fn head(ui: &mut egui::Ui, text: &str) {
            ui.add_space(4.0);
            ui.label(egui::RichText::new(text.to_uppercase()).font(prop(10.0)).color(theme::INK3));
            ui.add_space(6.0);
        }

        // 뒤 화면 어둡게(dim) + 클릭 차단 — 배경 클릭 시 닫는다(다른 모달과 동일 alpha-180 검정).
        let screen = ctx.screen_rect();
        let dim = egui::Area::new(egui::Id::new("help_dim"))
            .order(egui::Order::Middle)
            .fixed_pos(Pos2::ZERO)
            .show(ctx, |ui| {
                ui.painter().with_clip_rect(screen).rect_filled(screen, 0.0, Color32::from_black_alpha(180));
                ui.allocate_rect(screen, Sense::click_and_drag())
            });
        if dim.inner.clicked() {
            close = true;
        }

        let cmd = cmd_key();
        egui::Window::new("help_modal")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            // 높이 고정(0=자동이면 내부 ScrollArea와 순환 의존으로 뷰포트가 붕괴, 표가 잘림 #66).
            // 2열 표(~410px)+헤더/푸터가 들어가는 높이. 콘텐츠가 더 길면 ScrollArea가 스크롤한다.
            .fixed_size(Vec2::new(680.0, 560.0))
            .frame(modal_frame())
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                modal_header(
                    ui,
                    tr(lang, "단축키"),
                    tr(lang, "사진 위에서 바로 누르면 됩니다 — 입력창이 없어 즉시 반응"),
                );
                // 창 높이가 고정(아래 fixed_size)이라 ScrollArea가 실제 가용 높이를 받는다 — 창 높이를
                // 0(자동)으로 두면 ScrollArea 뷰포트↔창 높이가 순환 의존이 되어 뷰포트가 콘텐츠 최소값으로
                // 붕괴, 2열 표가 2행에서 잘렸다(#66). 내용<가용이면 스크롤 없이 전부 보이고, 넘치면 스크롤.
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        // 2열은 ui.columns로 나눈다(각 열 top_down, 내용에 맞춰 높이 증가).
                        ui.columns(2, |c| {
                            // ── 좌열: 이동 · 분류 · 별점·태그 ──
                            {
                                let ui = &mut c[0];
                                head(ui, tr(lang, "이동"));
                                row(ui, &["←", "→"], tr(lang, "이전·다음"));
                                row(ui, &["↑", "↓"], tr(lang, "그리드 줄 이동"));
                                row(ui, &[tr(lang, "휠")], tr(lang, "사진 넘김(창맞춤일 때)"));
                                row(ui, &["G"], tr(lang, "점프"));
                                row(ui, &["B"], tr(lang, "일괄 분류(그리드)"));

                                head(ui, tr(lang, "분류"));
                                row(ui, &["Q"], tr(lang, "채택"));
                                row(ui, &["W"], tr(lang, "보류"));
                                row(ui, &["E"], tr(lang, "제외"));
                                row(ui, &["R"], tr(lang, "해제"));
                                ui.label(
                                    egui::RichText::new(tr(lang, "같은 키 재입력 = 해제(토글)"))
                                        .font(mono(9.5))
                                        .color(theme::INK4),
                                );
                                // 컬링 되돌리기(#78).
                                row(ui, &[format!("{}Z", cmd).as_str()], tr(lang, "되돌리기"));
                                row(ui, &[format!("{}⇧Z", cmd).as_str()], tr(lang, "다시 실행"));

                                head(ui, tr(lang, "별점·태그"));
                                row(ui, &["1~5"], tr(lang, "별점"));
                                row(ui, &["`"], tr(lang, "별점 해제"));
                                row(ui, &["⇧1~5"], tr(lang, "컬러 태그"));
                                row(ui, &["⇧0"], tr(lang, "태그 해제"));
                            }
                            // ── 우열: 보기 · 확대 · 파일 ──
                            {
                                let ui = &mut c[1];
                                head(ui, tr(lang, "보기"));
                                row(ui, &["T"], tr(lang, "단일↔그리드"));
                                row(ui, &["D"], tr(lang, "원본(ORIG)"));
                                row(ui, &["I"], "EXIF");
                                row(ui, &["H"], tr(lang, "히스토그램"));
                                row(ui, &["M"], tr(lang, "촬영 위치 지도"));
                                row(ui, &["A"], tr(lang, "AF 포인트"));
                                row(ui, &["F"], tr(lang, "라벨 필터 순환"));
                                row(ui, &["F11"], tr(lang, "전체화면"));

                                head(ui, tr(lang, "확대"));
                                row(ui, &[tr(lang, "클릭"), "Space", "Z"], tr(lang, "창맞춤↔1:1"));
                                row(
                                    ui,
                                    &[format!("Ctrl+{}", tr(lang, "휠")).as_str(), tr(lang, "핀치")],
                                    tr(lang, "연속 확대"),
                                );
                                row(ui, &[tr(lang, "드래그")], tr(lang, "이동(확대 중)"));

                                head(ui, tr(lang, "파일"));
                                row(ui, &[format!("{}O", cmd).as_str()], tr(lang, "폴더 열기"));
                                row(ui, &[format!("{}E", cmd).as_str(), "Enter"], tr(lang, "전송"));
                                row(ui, &[tr(lang, "드래그")], tr(lang, "드래그앤드롭으로 폴더 열기"));
                            }
                        });
                    });

                // 푸터: ?·F1 열기 — Esc 닫기 (키캡 배지).
                ui.add_space(12.0);
                hline_full(ui);
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    kbd(ui, "?");
                    ui.add_space(3.0);
                    kbd(ui, "F1");
                    ui.add_space(5.0);
                    ui.label(egui::RichText::new(tr(lang, "열기")).font(mono(10.0)).color(theme::INK3));
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new("—").font(mono(10.0)).color(theme::INK4));
                    ui.add_space(10.0);
                    kbd(ui, "Esc");
                    ui.add_space(5.0);
                    ui.label(egui::RichText::new(tr(lang, "닫기")).font(mono(10.0)).color(theme::INK3));
                });
            });

        // 닫기: 배경 클릭(close) · Esc · ?(⇧/) · F1. 모달이 전역 키를 막으므로 여기서 직접 받는다.
        // NOTE: Key::Questionmark가 egui 0.29에 없으면 그 항만 지운다(Slash+shift·Esc·F1로 닫힘).
        let key_close = ctx.input(|i| {
            i.key_pressed(egui::Key::Escape)
                || i.key_pressed(egui::Key::F1)
                || i.key_pressed(egui::Key::Questionmark)
                || (i.key_pressed(egui::Key::Slash) && i.modifiers.shift)
        });
        if close || key_close {
            self.show_help = false;
        }
    }
}
