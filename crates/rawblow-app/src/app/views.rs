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

    /// 상단 툴바. 창이 좁으면 밀도 단계([`ToolbarTier`])를 내려 「(키)」 힌트를 툴팁으로 옮긴다.
    /// 단계는 창 폭·언어·내용의 최악 경우로만 정한다(자기 출력을 보지 않아 깜빡이지 않는다).
    /// 돌려주는 값은 실제로 그린 두 묶음의 끝이다(겹침 테스트용).
    pub(super) fn ui_toolbar(&mut self, ctx: &egui::Context) -> ToolbarLayout {
        let mut layout = ToolbarLayout::default();
        egui::TopBottomPanel::top("toolbar")
            .exact_height(TOOLBAR_H)
            .frame(egui::Frame::none().fill(theme::BG1).inner_margin(egui::Margin::symmetric(12.0, 6.0)))
            .show(ctx, |ui| {
                let avail = ui.available_rect_before_wrap();
                self.refresh_look_worst(ui);
                let tier = self.toolbar_tier(ui, avail.width());
                let (left, right) = toolbar_items(&self.toolbar_content(tier), tier);
                layout.tier = tier;
                layout.left_end = avail.left();
                layout.right_start = avail.right();
                // Tight는 위젯 사이 간격까지 줄인다(측정 tb_extent도 같은 값을 쓴다).
                let spacing = tb_spacing(ui, tier);
                ui.spacing_mut().item_spacing.x = spacing;
                ui.horizontal_centered(|ui| {
                    for item in &left {
                        let r = self.ui_toolbar_item(ui, item, &mut layout);
                        layout.left_end = layout.left_end.max(r.right());
                    }
                    // (필터 변경은 좌측 레일에서 전담 — 상단 Filter 버튼 제거, #toolbar feedback)
                    // 폴더 자동 분류(#34)는 좌측 레일 하단으로 이동(테스트 피드백) — 오른쪽은 전송/점프/일괄/⚙/?.
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        for item in &right {
                            let r = self.ui_toolbar_item(ui, item, &mut layout);
                            layout.right_start = layout.right_start.min(r.left());
                        }
                    });
                });
            });
        layout
    }

    /// 툴바 위젯 하나를 그리고 클릭을 처리한다. 돌려주는 사각형은 위젯이 차지한 자리다.
    fn ui_toolbar_item(&mut self, ui: &mut egui::Ui, item: &TbItem, layout: &mut ToolbarLayout) -> Rect {
        match item {
            TbItem::Logo => {
                // 로고 마크(작게).
                let (mr, _) = ui.allocate_exact_size(Vec2::splat(TB_LOGO), Sense::hover());
                crate::logo::draw_mark(ui.painter(), mr);
                ui.add_space(TB_LOGO_GAP);
                mr
            }
            TbItem::Sep => vsep(ui),
            TbItem::Toggle { label, tip, action } => {
                let resp = toggle_btn(ui, label, self.tb_active(*action));
                let resp = match tip {
                    Some(t) => resp.on_hover_text(t.as_str()),
                    None => resp,
                };
                if resp.clicked() {
                    self.tb_click(*action);
                }
                resp.rect
            }
            TbItem::Arrow { glyph, action } => {
                let resp = ui.add(egui::Button::new(*glyph).frame(false));
                if resp.clicked() {
                    self.tb_click(*action);
                }
                resp.rect
            }
            TbItem::Counter(text) => ui.label(counter_text(text)).rect,
            TbItem::Look(btn) => {
                let r = self.ui_look_button(ui, btn);
                layout.look = Some(r);
                r
            }
            TbItem::Accent { label, tip, action } => {
                let resp = ui.add(egui::Button::new(accent_text(label)).fill(theme::ACCENT));
                let resp = match tip {
                    Some(t) => resp.on_hover_text(t.as_str()),
                    None => resp,
                };
                if resp.clicked() {
                    self.tb_click(*action);
                }
                resp.rect
            }
        }
    }

    /// 툴바 토글이 켜져 보이는지.
    fn tb_active(&self, action: TbAction) -> bool {
        match action {
            TbAction::Single => self.view == ViewMode::Single,
            TbAction::Grid => self.view == ViewMode::Grid,
            TbAction::Fit => self.fit,
            // 1:1은 **실측** 기준이다(#48) — 표시 배율(원본 픽셀 대비)이 100%일 때 켜진다.
            // zoom을 그대로 보면 프리뷰/ORIG에서 같은 화면 크기가 다른 값이라 칩이 깜빡인다.
            TbAction::OneToOne => {
                !self.fit
                    && self
                        .current_real()
                        .and_then(|r| self.display_zoom(r))
                        .is_some_and(|z| (z - 1.0).abs() < 0.01)
            }
            TbAction::Orig => self.full_raw,
            TbAction::Full => self.fullscreen,
            TbAction::Exif => self.show_exif,
            TbAction::Hist => self.show_hist,
            TbAction::Guides => self.cfg.guides != GuidePreset::Off,
            TbAction::Settings => self.show_settings,
            TbAction::Help => self.show_help,
            TbAction::OpenFolder
            | TbAction::Prev
            | TbAction::Next
            | TbAction::Transfer
            | TbAction::Jump
            | TbAction::Bulk => false,
        }
    }

    fn tb_click(&mut self, action: TbAction) {
        match action {
            // 폴더 열기(이슈 #2: 폴더 연 상태에서 다른 폴더 여는 버튼 없음 — ⌘O를 못 찾음).
            TbAction::OpenFolder => self.pick_folder(),
            TbAction::Single => self.view = ViewMode::Single,
            TbAction::Grid => self.view = ViewMode::Grid,
            TbAction::Prev => self.advance(-1),
            TbAction::Next => self.advance(1),
            TbAction::Fit => {
                self.fit = true;
                self.pan = Vec2::ZERO;
            }
            TbAction::OneToOne => {
                self.request_one_to_one();
                self.pan = Vec2::ZERO;
            }
            TbAction::Orig => self.full_raw = !self.full_raw,
            // 전체화면 토글(#31). 버튼/F11 어느 쪽이든 OS 창 풀스크린으로(update에서 동기화).
            TbAction::Full => self.fullscreen = !self.fullscreen,
            TbAction::Exif => self.show_exif = !self.show_exif,
            TbAction::Hist => self.show_hist = !self.show_hist,
            TbAction::Guides => self.cycle_guides(),
            TbAction::Transfer => self.open_transfer(),
            TbAction::Jump => self.jump_open = true,
            // 그리드 모드 한정: 파일명으로 일괄 라벨링(#3).
            TbAction::Bulk => {
                self.bulk_open = true;
                self.bulk_searched = false;
                self.bulk_hits.clear();
            }
            TbAction::Settings => self.open_settings(),
            // 단축키 치트시트(#66): ⚙ 옆 작은 ? 토글(⚙과 같은 스타일). ?/F1로도 여닫는다.
            TbAction::Help => self.show_help = !self.show_help,
        }
    }

    /// 지금 화면에 그릴 툴바 내용.
    fn toolbar_content(&self, tier: ToolbarTier) -> TbContent {
        let f = self.filtered();
        TbContent {
            lang: self.lang,
            view: self.view,
            counter: counter_label((self.index + 1).min(f.len().max(1)), f.len()),
            guide: tr(self.lang, self.cfg.guides.label_ko()),
            look: look_button(&self.cfg.look, self.look_compare, tier),
        }
    }

    /// 밀도 단계. 넓은 단계부터 최악 내용이 들어가는지 본다. 단일 보기와 그리드 중 넓은 쪽을 써서
    /// T로 보기를 바꿔도 단계가 같다. 어느 것도 안 들어가면 가장 좁은 Tight.
    pub(super) fn toolbar_tier(&self, ui: &egui::Ui, avail: f32) -> ToolbarTier {
        [ToolbarTier::Full, ToolbarTier::NoHints, ToolbarTier::Compact]
            .into_iter()
            .find(|&tier| self.toolbar_need(ui, tier) + TB_MIN_GAP <= avail)
            .unwrap_or(ToolbarTier::Tight)
    }

    /// 이 단계에서 최악 내용이 차지하는 폭(왼쪽 묶음 + 오른쪽 묶음, 사이 여백 제외).
    /// 최악 내용: 이 폴더 장수의 카운터, 가장 넓은 구도 이름, 나올 수 있는 가장 넓은 LUT 라벨.
    pub(super) fn toolbar_need(&self, ui: &egui::Ui, tier: ToolbarTier) -> f32 {
        let lang = self.lang;
        // 카운터는 「{:03} / 전체」라 위치·필터와 상관없이 전체 장수의 자릿수가 상한이다.
        let n = self.items.len().max(999);
        let counter = counter_label(n, n);
        let guide = GUIDE_PRESETS
            .iter()
            .map(|g| tr(lang, g.label_ko()))
            .max_by(|a, b| {
                let w = |s: &str| tb_galley(ui, toggle_btn_text(s, false)).size().x;
                w(a).total_cmp(&w(b))
            })
            .unwrap_or("");
        let look = self.look_button_worst(ui, tier);
        [ViewMode::Single, ViewMode::Grid]
            .into_iter()
            .map(|view| {
                let c = TbContent { lang, view, counter: counter.clone(), guide, look: look.clone() };
                let (left, right) = toolbar_items(&c, tier);
                let spacing = tb_spacing(ui, tier);
                tb_extent(ui, &left, spacing) + tb_extent(ui, &right, spacing)
            })
            .fold(0.0, f32::max)
    }

    /// 이 단계에서 LUT 버튼이 가질 수 있는 가장 넓은 모양. 글자는 [`Self::refresh_look_worst`]가
    /// 골라 둔 것을 쓴다(프레임마다 폴더 파일 전부를 다시 재지 않게).
    fn look_button_worst(&self, ui: &egui::Ui, tier: ToolbarTier) -> LookBtn {
        let widest = if tier >= ToolbarTier::Compact {
            "LUT".to_string()
        } else {
            match &self.look_worst {
                Some(w) => w.text.clone(),
                None => self.pick_look_worst(ui).0,
            }
        };
        LookBtn { text: widest.clone(), state: LookState::Off, widths: vec![widest], tip_head: None }
    }

    /// 단계 계산용 가장 넓은 LUT 글자를 (언어, 폴더 목록, 룩 경로)가 바뀔 때만 다시 고른다.
    /// 룩 메뉴가 열려 있는 동안은 바꾸지 않는다 — 메뉴를 열 때 폴더를 다시 읽어 목록이 길어져도
    /// 열린 메뉴와 LUT 버튼이 옆으로 뛰지 않게. 닫힌 뒤 다음 프레임에 반영된다.
    pub(super) fn refresh_look_worst(&mut self, ui: &egui::Ui) {
        let key = (self.lang, self.lut_gen, self.cfg.look.cube_path.clone());
        if let Some(w) = &self.look_worst {
            let open = ui.memory(|m| m.is_popup_open(egui::Id::new(LOOK_MENU_ID)));
            if open || w.key == key {
                return;
            }
        }
        let (text, floor) = self.pick_look_worst(ui);
        self.look_label_floor = floor;
        self.look_worst = Some(LookWorst { key, text });
    }

    /// 후보(비교 글자·빌트인·폴더 목록·룩 경로·세션 동안 본 가장 넓은 룩 경로) 중 가장 넓은 글자와,
    /// 갱신한 세션 최댓값. 룩 경로는 이름이 빌트인이어도 넣는다 — 메뉴가 그 파일 줄을 계속 보이므로.
    fn pick_look_worst(&self, ui: &egui::Ui) -> (String, Option<String>) {
        let w = |t: &str| tb_galley(ui, look_btn_text(t, theme::INK)).size().x;
        let cur = (!self.cfg.look.cube_path.is_empty())
            .then(|| look_on_label(&short_stem(Path::new(&self.cfg.look.cube_path), LOOK_BTN_STEM)));
        let floor = match (cur, &self.look_label_floor) {
            (Some(c), Some(f)) if w(f) >= w(&c) => Some(f.clone()),
            (Some(c), _) => Some(c),
            (None, f) => f.clone(),
        };
        let candidates = std::iter::once(LOOK_COMPARE.to_string())
            .chain(rawblow_core::lut::BUILTINS.iter().map(|b| look_on_label(b)))
            .chain(self.lut_files.iter().map(|p| look_on_label(&short_stem(p, LOOK_BTN_STEM))))
            .chain(floor.clone());
        let mut widest = "LUT".to_string();
        let mut best = w(&widest);
        for c in candidates {
            let cw = w(&c);
            if cw > best {
                best = cw;
                widest = c;
            }
        }
        (widest, floor)
    }

    /// 툴바 LUT 버튼과 룩 메뉴. 단일 보기 툴바에서만 그린다.
    fn ui_look_button(&mut self, ui: &mut egui::Ui, btn: &LookBtn) -> Rect {
        let lang = self.lang;
        let menu_id = egui::Id::new(LOOK_MENU_ID);
        let (rect, resp) = ui.allocate_exact_size(look_btn_size(ui, btn), Sense::click());
        // 모달 아래에서는 누를 수 없다. 그리지 않은 것으로 치므로 열린 메뉴는 ui_screen이 닫는다.
        let interactive = !self.has_modal();
        if interactive {
            self.look_button_drawn = true;
        }
        paint_look_button(ui, rect, &resp, btn);
        if resp.hovered() && interactive {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let cube = (self.cfg.look.name == "file" && !self.cfg.look.cube_path.is_empty())
            .then(|| self.cfg.look.cube_path.clone());
        let head = btn.tip_head.clone();
        let resp = resp.on_hover_ui(|ui| {
            if let Some(h) = head {
                ui.label(h);
            }
            // prop 폰트라 백슬래시 키 이름을 넣지 않는다(윈도우 맑은 고딕은 ₩로 그린다).
            ui.label(tr(lang, "룩(LUT) 고르기 · 히스토그램은 룩 적용 전"));
            if let Some(path) = cube {
                ui.label(egui::RichText::new(nfc_hangul(&path)).font(mono(11.0)));
            }
        });
        if !interactive {
            return rect;
        }
        if resp.clicked() {
            if !ui.memory(|m| m.is_popup_open(menu_id)) {
                // 폴더에 파일을 넣고 바로 메뉴를 열면 보이게, 열 때마다 다시 읽는다.
                self.reload_lut_files();
            }
            ui.memory_mut(|m| m.toggle_popup(menu_id));
        }
        // CloseOnClick은 메뉴 안의 스크롤바·캡션을 눌러도 닫으므로 바깥 클릭만 닫게 하고,
        // 항목을 고르면 아래에서 직접 닫는다.
        let pick = egui::popup_below_widget(ui, menu_id, &resp, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
            self.look_menu(ui)
        })
        .flatten();
        if pick.is_some() {
            ui.memory_mut(|m| m.close_popup());
        }
        let ctx = ui.ctx().clone();
        match pick {
            Some(LookPick::Name(name)) => {
                if self.cfg.look.name != name {
                    self.set_look_name(&name, &ctx);
                }
            }
            Some(LookPick::File(path)) => {
                let same = self.cfg.look.name == "file" && same_cube(Path::new(&self.cfg.look.cube_path), &path);
                if !same {
                    // 실패하면 set_cube_path가 토스트를 띄우고 이전 룩을 둔다.
                    self.set_cube_path(&path, &ctx);
                }
            }
            Some(LookPick::Settings) => {
                self.settings_focus_lut = true;
                self.open_settings();
            }
            None => {}
        }
        rect
    }

    /// 룩 메뉴 내용. 고른 것을 돌려주고 적용은 부르는 쪽이 한다.
    fn look_menu(&self, ui: &mut egui::Ui) -> Option<LookPick> {
        let lang = self.lang;
        ui.set_min_width(LOOK_MENU_MIN_W);
        // 좁은 메뉴에서 이름이 일찍 줄바꿈되지 않게(ComboBox와 같은 설정).
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
        let mut pick = None;
        let look = &self.cfg.look;
        if ui.selectable_label(look_name(look).is_none(), tr(lang, "없음")).clicked() {
            pick = Some(LookPick::Name(String::new()));
        }
        ui.separator();
        for b in rawblow_core::lut::BUILTINS {
            if ui.selectable_label(look.name == b, b).clicked() {
                pick = Some(LookPick::Name(b.to_string()));
            }
        }
        ui.separator();
        // 룩 경로(폴더 밖일 수 있다). 이름이 빌트인이어도 경로는 남아 있어 그 파일로 돌아갈 수 있게 줄을 둔다.
        // 선택 표시는 지금 룩이 그 파일일 때만.
        let cur_path = (!look.cube_path.is_empty()).then(|| Path::new(&look.cube_path));
        let cur_file = cur_path.filter(|_| look.name == "file");
        // 폴더 경로는 NFC로 적어 두고 옛 「LUT」 버튼은 디스크 그대로(NFD)를 저장했으므로 NFC로 비교한다.
        let outside = cur_path.filter(|cur| !self.lut_files.iter().any(|p| same_cube(p, cur)));
        let mut file_row = |ui: &mut egui::Ui, path: &Path| {
            let on = cur_file.is_some_and(|cur| same_cube(cur, path));
            let resp = ui.selectable_label(on, short_stem(path, LOOK_MENU_STEM)).on_hover_ui(|ui| {
                ui.label(egui::RichText::new(nfc_path_label(path)).font(mono(11.0)));
            });
            if resp.clicked() {
                pick = Some(LookPick::File(path.to_path_buf()));
            }
        };
        if !self.lut_files.is_empty() {
            // 목록 아래 고정 줄(폴더 밖 줄·구분선·「LUT 폴더 설정…」·구분선·안내 줄)과 팝업 아래 여백을 남기고
            // 화면 바닥까지만 쓴다. 넘치면 egui가 팝업을 위로 밀어 LUT 버튼을 덮는다.
            let row = ui.spacing().interact_size.y + ui.spacing().item_spacing.y;
            let sep = LOOK_MENU_SEP_H + ui.spacing().item_spacing.y;
            let rows_below = if outside.is_some() { 3.0 } else { 2.0 };
            let tail = rows_below * row + 2.0 * sep + LOOK_MENU_TAIL_PAD;
            let room = ui.ctx().screen_rect().bottom() - ui.cursor().top() - tail;
            egui::ScrollArea::vertical()
                .id_salt("look_menu_files")
                .max_height(room.clamp(LOOK_MENU_LIST_MIN_H, LOOK_MENU_LIST_H))
                .show(ui, |ui| {
                    for path in &self.lut_files {
                        file_row(ui, path);
                    }
                });
        }
        if let Some(cur) = outside {
            file_row(ui, cur);
        }
        let caption = match self.lut_dir_status {
            rawblow_core::lut::CubeDir::Unset => Some("설정에서 LUT 폴더를 지정하면 .cube 파일이 여기 나옵니다"),
            rawblow_core::lut::CubeDir::Empty => Some("LUT 폴더에 .cube 파일이 없습니다"),
            rawblow_core::lut::CubeDir::Missing => Some("LUT 폴더를 찾을 수 없습니다"),
            rawblow_core::lut::CubeDir::Unreadable => Some("LUT 폴더를 읽을 수 없습니다(접근 권한)"),
            rawblow_core::lut::CubeDir::Found { .. } => None,
        };
        if let Some(c) = caption {
            // 폭 고정 줄바꿈 — 메뉴가 캡션 길이만큼 넓어지지 않게.
            let galley = ui.fonts(|f| f.layout(tr(lang, c).to_string(), prop(11.5), theme::INK3, LOOK_MENU_MIN_W));
            ui.add_space(2.0);
            ui.label(galley);
            ui.add_space(2.0);
        }
        ui.separator();
        if ui.selectable_label(false, tr(lang, "LUT 폴더 설정…")).clicked() {
            pick = Some(LookPick::Settings);
        }
        ui.separator();
        // 선택 항목처럼 보이지 않는 안내 줄.
        ui.horizontal(|ui| {
            kbd(ui, "\\");
            ui.add_space(4.0);
            ui.label(egui::RichText::new(tr(lang, "누르고 있는 동안 룩 끄기")).font(mono(9.5)).color(theme::INK4));
        });
        pick
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
                        self.apply_label_filter(filt);
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
                    let sf = if sf == self.star_filter { StarFilter::Any } else { sf };
                    self.apply_star_filter(sf);
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
                    self.apply_tag_filter(tf);
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
        // 원본 픽셀 대비 배율(#48). 100%가 실측 1:1이며, D로 프리뷰↔ORIG를 오가도 사진 크기가
        // 그대로면 숫자도 그대로다(zoom을 그대로 쓰면 텍스처 기준이라 100%↔23%로 튀었다).
        let zoom_pct = self.current_real().and_then(|r| self.display_zoom(r)).map(|z| z * 100.0);
        // 보기 방식 · 화면 출력 색(단일 보기만). 예전에는 사진 HUD 카운터 아래 꼬리에 있었다.
        let view_text = (self.view == ViewMode::Single).then(|| {
            let real = self.current_real();
            status_view_text(
                self.lang,
                real.is_some_and(|r| self.thumb_only(r)),
                self.full_raw,
                real.is_some_and(|r| self.orig_fallback.contains(&r)),
                self.output_kind(),
            )
        });
        egui::TopBottomPanel::bottom("status")
            .exact_height(STATUS_H)
            .frame(egui::Frame::none().fill(theme::BG2).inner_margin(egui::Margin::symmetric(12.0, 4.0)))
            .show(ctx, |ui| {
                // 오른쪽 묶음(오른쪽 끝부터, right_to_left): (글자, 툴팁). 폭 재기와 그리기가 같은 목록을 쓴다.
                // 맨 오른쪽: 자동저장 상태(레일에서 이동). 글자(우) 왼쪽에 상태 점을 둬 "● saved"/"● saving…"로
                // 보이게 한다. 저장 실패(#62)가 최우선: 실패 중에도 dirty가 유지된 채 재시도하므로
                // "saving…"으로 보이면 거짓 안심이다. 원인(원본 오류 문자열)과 대상 폴더는 hover로(상태바 폭 유지).
                let (dot, txt) = if self.save_error.is_some() {
                    (theme::REJECT, tr(self.lang, "저장 실패"))
                } else if self.sidecar_dirty {
                    (theme::HOLD, "saving…")
                } else {
                    (theme::OK, "saved")
                };
                let save_tip = self.save_error.as_ref().map(|err| {
                    let dir = self.folder.as_deref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
                    format!("{dir}\n{err}")
                });
                let sep = || (egui::RichText::new("·").font(mono(10.5)).color(theme::INK4), None);
                let mut right: Vec<(egui::RichText, Option<String>)> = vec![
                    (egui::RichText::new(txt).font(mono(10.5)).color(theme::INK3), save_tip),
                    (egui::RichText::new("●").color(dot).size(8.0), None),
                ];
                // 그 왼쪽: 배율(단일/전체화면에서만 의미).
                if self.view == ViewMode::Single || self.fullscreen {
                    let zoom = match zoom_pct {
                        Some(p) => format!("{p:.0}%"),
                        None => "—".to_string(),
                    };
                    right.push(sep());
                    right.push((egui::RichText::new(zoom).font(mono(10.5)).color(theme::INK2), None));
                }
                // 맨 왼쪽: 보기 방식 · 출력 색 → 「FIT · Display P3 · 12% · ● saved」.
                if let Some(text) = &view_text {
                    right.push(sep());
                    right.push((
                        egui::RichText::new(text).font(mono(10.5)).color(theme::INK3),
                        Some(tr(self.lang, "보기 방식 · 화면 출력 색").to_string()),
                    ));
                }
                ui.horizontal_centered(|ui| {
                    ui.label(egui::RichText::new("●").color(theme::OK).size(8.0));
                    ui.label(egui::RichText::new(format!("READY · {} ITEMS · {}/{}", self.items.len(), (self.index + 1).min(f_len.max(1)), f_len)).font(mono(10.5)).color(theme::INK3));
                    ui.label(egui::RichText::new(format!("· P {pick} H {hold} X {reject} · {unrated}")).font(mono(10.5)).color(theme::INK3));
                    // 로딩·프레임 통계: 우측 → 좌측으로 이동(테스트 피드백). 오른쪽 묶음(저장·배율·보기 문구)이
                    // 우선이라, 좁은 창에 장수가 많으면 이 줄만 남는 폭에 맞춰 줄임표로 자른다(전문은 hover).
                    let spacing = ui.spacing().item_spacing.x;
                    let right_w: f32 = right
                        .iter()
                        .map(|(t, _)| {
                            egui::WidgetText::from(t.clone())
                                .into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, egui::TextStyle::Body)
                                .size()
                                .x
                                + spacing
                        })
                        .sum();
                    let room = (ui.available_width() - right_w - STATUS_MIN_GAP).max(0.0);
                    let stats = egui::RichText::new(format!("· {:.1}ms · {:.0} FPS · GPU wgpu · PRELOAD ±{}", self.frame_ms, fps, self.cfg.preload))
                        .font(mono(10.5))
                        .color(theme::INK4);
                    ui.allocate_ui(Vec2::new(room, ui.available_height()), |ui| {
                        ui.add(egui::Label::new(stats).truncate());
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        for (text, tip) in right {
                            let resp = ui.label(text);
                            if let Some(tip) = tip {
                                resp.on_hover_text(tip);
                            }
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
                            self.keep_view_mode_on_move(); // 화살표 이동과 같은 규칙(#85/#87)
                        }
                    }
                });
            });
    }

    pub(super) fn ui_single(&mut self, ui: &mut egui::Ui) {
        let rect = ui.max_rect();
        match self.current_real() {
            None => self.ui_empty_state(ui),
            Some(real) => {
                self.photo_view(ui, rect, real);
                if !self.has_modal() {
                    // 카운터 아래 꼬리는 경고만. 보기 방식·출력 색은 상태 바로 옮겼다.
                    let warn = single_hud_warning(
                        self.lang,
                        self.thumb_only(real),
                        self.full_raw,
                        self.orig_fallback.contains(&real),
                    );
                    self.paint_hud(ui, rect, real, warn);
                    self.paint_thumb_only_banner(ui, rect, real);
                    self.ui_map_overlay(ui, rect, real);
                }
            }
        }
        // 룩 메뉴가 열려 있으면 사진 위에 클릭 방패를 둔다. 메뉴를 닫으려는 클릭이 Fit↔1:1·재시도·
        // 지도(브라우저)·±로 새지 않게. click 전용이라 드래그 팬과 Ctrl+휠 줌은 그대로 사진이 받는다.
        // 사진·배너·지도보다 뒤에 등록해야 위에 놓인다.
        if ui.memory(|m| m.is_popup_open(egui::Id::new(LOOK_MENU_ID))) {
            let _ = ui.interact(rect, ui.id().with("look_menu_shield"), Sense::click());
        }
    }

    /// 본 이미지 디코딩이 끝내 실패했는데 썸네일만 있는 상태(#114). 이때 photo_view는 썸네일을
    /// 확대해 그리므로, 사진처럼 조용히 보이지 않게 알려야 한다.
    fn thumb_only(&self, real: usize) -> bool {
        !self.cache.contains(real) && self.thumbs.contains(real) && self.decode_dead(real)
    }

    /// thumb_only면 상단(HUD 파일명·카운터 줄 아래)에 경고 배너. 클릭하면 재시도(#75와 같은 수단).
    fn paint_thumb_only_banner(&mut self, ui: &mut egui::Ui, rect: Rect, real: usize) {
        if !self.thumb_only(real) {
            return;
        }
        let msg = format!(
            "{} · {}",
            tr(self.lang, "⚠ 본 이미지를 열 수 없어 썸네일을 확대해 보여 주고 있습니다"),
            tr(self.lang, "클릭하여 재시도")
        );
        let pos = Pos2::new(rect.center().x, rect.top() + 72.0);
        let galley = ui.painter().layout_no_wrap(msg, mono(12.0), theme::WARN);
        let bg = Rect::from_center_size(pos, galley.size() + Vec2::new(24.0, 12.0));
        ui.painter().rect_filled(bg, Rounding::same(6.0), Color32::from_black_alpha(210));
        ui.painter().galley(bg.center() - galley.size() / 2.0, galley, theme::WARN);
        let resp = ui.interact(bg, ui.id().with(("retry_thumb_only", real)), Sense::click());
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if resp.clicked() {
            self.retry_decode(real);
            ui.ctx().request_repaint();
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
            self.view_mag = ViewMag::default(); // 유지할 화면상 배율·상한 기준도 항목마다 새로(#48)
            self.view_ref_long = 0.0;
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
                    let name = nfc_file_name(&self.items[real].entry.display);
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
        // #48: 확대 상한은 현재 텍스처가 아니라 **이 항목에서 본 가장 좋은 해상도**를 기준으로
        // 잡는다. 상한을 현재 텍스처 기준(8x 텍스처픽셀)으로 두면 같은 화면상 배율이 프리뷰
        // (1920)에서는 표현 불가라, ORIG(8192)에서 약 1.9배 넘게 확대한 뒤 D로 돌아올 때 상한에
        // 잘리고 그 잘린 값이 새 기준이 되어 배율이 영구히 깎였다 — #48 회귀의 직접 원인.
        // one_to_one = 기준 해상도의 1:1이 현재 텍스처에서는 몇 배인지(프리뷰에선 1보다 크다).
        // 기준은 원본 실측(`ref_long_px`)을 쓴다 — "본 것 중 가장 큼"(view_ref_long)으로 잡으면
        // ORIG를 아직 못 본 프리뷰 단계의 상한이 1:1(=1.0)에 묶여, 실측 1:1조차 표현하지 못한다.
        let cur_long = size.max_elem().max(1.0);
        self.view_ref_long = self.view_ref_long.max(cur_long);
        let ref_long = self.ref_long_px(real).unwrap_or(self.view_ref_long);
        let one_to_one = (ref_long / cur_long).max(1.0);
        let min_zoom = fit_scale.min(1.0);
        let max_zoom = (8.0 * one_to_one).max(fit_scale); // 기준 해상도로 8x(작은 이미지도 확대 가능)

        // #85: 넘어온 확대 상태를 이 사진에 복원한다. `mag`는 긴 변이 차지하는 화면 px이라
        // 해상도·방향에 불변이다 — 가로(1920x1280) → 세로(1280x1920)로 넘어가도 긴 변은 그대로
        // 1920이라 픽셀 확인 배율이 똑같이 유지된다. pan은 표시 크기 대비 비율로 되돌린다.
        // 새 사진의 창맞춤 배율이 더 크면(작은 이미지) 조용히 창맞춤으로 내려앉는다.
        let restored = self.zoom_restore;
        if restored {
            if let Some((mag, pan_norm)) = self.keep_zoom {
                let want = mag / cur_long;
                let z = want.clamp(min_zoom, max_zoom);
                if z <= fit_scale * 1.001 {
                    self.fit = true;
                    self.pan = Vec2::ZERO;
                } else {
                    self.zoom = z;
                    self.pan = Vec2::new(pan_norm.x * size.x * z, pan_norm.y * size.y * z);
                    // #48: 넘겨받은 배율이 지금 텍스처(보통 먼저 도착한 1920 프리뷰)에서 표현
                    // 불가라 잘렸더라도 기준으로는 **잘리기 전 값**을 남긴다. 잘린 값을 기준으로
                    // 삼으면 곧 도착할 ORIG에서도 되살아나지 못해 배율이 영구히 깎인다 —
                    // 아래 D 토글 경로에서 없앤 것과 같은 함정이 이 이월 경로에도 있었다.
                    self.view_mag.mag = Some(mag);
                    self.view_mag.pin = ((z - want).abs() > 1e-4).then_some(z);
                }
            }
            // 프리뷰(정식 해상도)가 도착했으면 복원 확정. 그 전까지는 썸네일 크기에 맞춘
            // 임시 값이라 매 프레임 다시 맞춘다.
            if self.fit || self.cache.contains(real) {
                self.zoom_restore = false;
            }
        }

        // #48: 실측 1:1 요청 소비 — 원본 1픽셀이 화면 1픽셀이 되도록 맞춘다(표시 배율 100%).
        // 기준(view_mag)에는 잘리기 전 목표를 남겨, 아직 프리뷰뿐이라 상한에 걸렸더라도 곧
        // 도착할 ORIG에서 제 배율이 나오게 한다.
        //
        // 기준이 확정되기 전(메타 로딩 중)에는 **소비하지 않고 기다린다**. 여기서 임시 기준으로
        // 계산해 버리면 그 값이 view_mag에 굳어, 곧 도착할 실측 크기와 어긋난 배율이 남는다.
        if self.one_to_one_pending && !self.ref_long_settled(real) {
            // 메타(원본 크기)를 기다리는 중 — 도착하면 다음 프레임에 적용한다.
            ui.ctx().request_repaint_after(Duration::from_millis(80));
        }
        if self.one_to_one_pending && self.ref_long_settled(real) {
            self.one_to_one_pending = false;
            if let Some(ref_long) = self.ref_long_px(real) {
                let want = ref_long / cur_long;
                let z = want.clamp(min_zoom, max_zoom);
                self.fit = false;
                self.zoom = z;
                self.view_mag.mag = Some(ref_long);
                self.view_mag.pin = ((z - want).abs() > 1e-4).then_some(z);
                self.zoom_restore = false; // 사용자가 지정한 배율 → #85 복원 대기 취소
            }
        }

        // #48: 같은 항목에서 표시 해상도가 바뀌면(ORIG 로드/언로드) 화면상 배율·보던 위치를 유지한다.
        // 유지 기준은 zoom(=화면픽셀/**텍스처**픽셀 → 해상도가 K배면 같은 값이 K배 확대를 뜻함)이
        // 아니라 **긴 변이 차지하는 화면 px**(view_mag)이다. 긴 변 기준이라 가로↔세로가 바뀌어도
        // (캐시된 옛 방향 → 새로 회전된 프리뷰) 종횡비만큼 배율이 튀지 않고, scaled(=size*zoom,
        // 화면상 크기)가 보존되므로 pan(화면픽셀)도 그대로 들어맞는다.
        //
        // 옛 구현은 zoom에 prev/cur 비율을 곱하고 매번 클램프해서, 한 번 상·하한에 잘리면 그 값이
        // 새 기준이 되어 배율이 **영구히** 깎였다(#48 회귀). 이제 잘린 프레임에서는 view_mag를
        // 덮지 않고 붙들어(`ViewMag::pin`) 두므로, ORIG에서 표현할 수 없는 배율(창보다 작은 프리뷰의
        // 1:1 등)이면 그 프레임만 경계로 올라붙고 D로 되돌아오면 원래 배율이 살아난다.
        // 복원 프레임(#85)에서는 zoom을 절대값으로 방금 정했으므로 건너뛴다.
        if !self.fit && !restored {
            self.zoom = sync_view_mag(
                &mut self.view_mag,
                self.zoom,
                cur_long,
                self.last_view_size.map(|prev| prev.max_elem()),
                min_zoom,
                max_zoom,
            );
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

        // 클릭(드래그 아님): fit ↔ 1:1(실측 기준, #48). 다음 프레임에 소비되므로 재페인트를 건다.
        if resp.clicked() {
            if self.fit {
                self.request_one_to_one();
                self.pan = Vec2::ZERO;
                ui.ctx().request_repaint();
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
        // 구도선은 사진 바로 위, AF·HUD·지도보다 아래(#90).
        self.paint_guides(ui, area, target);

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

    /// 표시된 사진 사각형에 구도선을 그린다. `target`은 회전·확대·이동이 반영된 영역.
    fn paint_guides(&self, ui: &egui::Ui, area: Rect, target: Rect) {
        if target.width() < 1.0 || target.height() < 1.0 {
            return;
        }
        let g = &self.cfg.guide_custom;
        let lines = rawblow_core::guides::guide_lines(
            self.cfg.guides,
            target.width(),
            target.height(),
            &g.vertical,
            &g.horizontal,
        );
        if lines.is_empty() {
            return;
        }
        let (weight, opacity) = rawblow_core::guides::clamp_guide_style(g.weight, g.opacity);
        let alpha = (opacity * 255.0).round().clamp(0.0, 255.0) as u8;
        let color = Color32::from_rgba_unmultiplied(g.rgb[0], g.rgb[1], g.rgb[2], alpha);
        let stroke = Stroke::new(weight, color);
        let p = ui.painter().with_clip_rect(area);
        for t in &lines.vertical {
            let x = target.min.x + t * target.width();
            p.line_segment([Pos2::new(x, target.min.y), Pos2::new(x, target.max.y)], stroke);
        }
        for t in &lines.horizontal {
            let y = target.min.y + t * target.height();
            p.line_segment(
                [Pos2::new(target.min.x, y), Pos2::new(target.max.x, y)],
                stroke,
            );
        }
    }

    /// `counter_suffix`는 카운터 아래 작은 꼬리(단일 보기는 경고, 전체화면은 「FULLSCREEN · ESC」).
    /// None이면 그 줄을 그리지 않는다.
    pub(super) fn paint_hud(&self, ui: &egui::Ui, area: Rect, real: usize, counter_suffix: Option<&str>) {
        let lang = self.lang;
        let it = &self.items[real];
        let f = self.filtered();
        // TL: 라벨 + 파일명.
        let name = nfc_file_name(&it.entry.display);
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
        if let Some(suffix) = counter_suffix {
            hud_text(ui, tr + Vec2::new(0.0, 34.0), Align2::RIGHT_TOP, suffix, mono(10.0), theme::INK3);
        }

        // BL: AI 컬링 판정 근거(#91) + EXIF. 근거는 판정·핵심 사유를 늘 보이고,
        // EXIF 오버레이(I)를 켜면 참고 사유(통과 점수·그룹 순위)까지 펼친다.
        let mut y = area.bottom() - 16.0;
        if self.show_exif {
            if let Some(ex) = &it.exif {
                let lines = exif_lines(ex);
                for line in lines.iter().rev() {
                    hud_text(ui, Pos2::new(area.left() + 16.0, y), Align2::LEFT_BOTTOM, line, mono(12.0), theme::INK2);
                    y -= 18.0;
                }
                y -= 8.0;
            }
        }
        if let Some(note) = &it.cull_note {
            let hint = if self.show_exif {
                crate::i18n::tr(lang, "AI 제안일 뿐 확정 판정이 아닙니다 — 최종 선택은 직접 확인하세요")
            } else {
                crate::i18n::tr(lang, "I: 판정 근거 자세히")
            };
            hud_text(ui, Pos2::new(area.left() + 16.0, y), Align2::LEFT_BOTTOM, hint, mono(10.0), theme::INK3);
            y -= 16.0;
            let lines = culling::cull_note_lines(lang, note, self.show_exif);
            for line in lines.iter().rev() {
                hud_text(ui, Pos2::new(area.left() + 16.0, y), Align2::LEFT_BOTTOM, line, mono(12.0), theme::INK2);
                y -= 18.0;
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
                            self.keep_view_mode_on_move(); // 화살표 이동과 같은 규칙(#85/#87)
                        }
                        if resp.double_clicked() {
                            self.index = fi;
                            self.keep_view_mode_on_move();
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
        // 다른 사진이면 옛 지도·좌표는 합성 대기 중에도 보이지 않게 바로 버린다. 인덱스만으론
        // 폴더 전환·재정렬 뒤 같은 자리의 다른 사진과 구분이 안 돼 generation까지 본다.
        let gen = self.generation;
        if self.map_state.as_ref().is_some_and(|m| m.gen != gen || m.real != real) {
            self.map_state = None;
        }
        // 항목·줌이 바뀌면 새로 합성. 단, 시작은 디코딩 유휴 시에만(사진 표시가 우선, #33 패턴).
        let stale = self.map_state.as_ref().map(|m| m.zoom != zoom).unwrap_or(true);
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
                    gen: self.generation,
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

    /// 전체화면. 룩 고르기 UI는 두지 않는다(룩 적용과 `\` 비교는 그대로 동작).
    pub(super) fn ui_fullscreen(&mut self, ctx: &egui::Context) {
        let bg = self.photo_bg();
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(bg))
            .show(ctx, |ui| {
                let rect = ui.max_rect();
                if let Some(real) = self.current_real() {
                    self.photo_view(ui, rect, real);
                    self.paint_hud(ui, rect, real, Some("FULLSCREEN · ESC"));
                    self.paint_thumb_only_banner(ui, rect, real);
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

/// 상태 바 오른쪽 보기 문구(단일 보기): 「{MODE} · {출력}」. 배율 왼쪽에 붙는다.
/// MODE는 썸네일 대체면 THUMB, ORIG를 켰는데 원본을 못 구했으면 PREVIEW, 그 밖은 ORIG/FIT.
/// 출력은 이 창의 실제 출력이라 설정 「현재 출력」과 같은 말을 쓴다.
pub(super) fn status_view_text(
    lang: Lang,
    thumb_only: bool,
    full_raw: bool,
    orig_fallback: bool,
    out: rawblow_core::color::OutputKind,
) -> String {
    let mode = if thumb_only {
        "THUMB"
    } else if full_raw && orig_fallback {
        "PREVIEW"
    } else if full_raw {
        "ORIG"
    } else {
        "FIT"
    };
    format!("{mode} · {}", output_short(lang, out))
}

/// 단일 보기 카운터 아래 꼬리. 경고만 남는다. 없으면 꼬리 줄을 그리지 않는다.
fn single_hud_warning(lang: Lang, thumb_only: bool, full_raw: bool, orig_fallback: bool) -> Option<&'static str> {
    if thumb_only {
        Some(tr(lang, "썸네일 · 본 이미지 열기 실패"))
    } else if full_raw && orig_fallback {
        Some(tr(lang, "PREVIEW · 원본 없음"))
    } else {
        None
    }
}

// ── 툴바 밀도와 LUT 버튼(#125 후속) ─────────────────────────────

/// 룩 메뉴 팝업 id. handle_keys(Esc·휠 가드)·사진 클릭 방패·고아 정리(ui_screen)가 같은 id를 본다.
pub(super) const LOOK_MENU_ID: &str = "look_menu";
/// 룩 메뉴 최소 폭(pt).
const LOOK_MENU_MIN_W: f32 = 200.0;
/// 메뉴의 LUT 폴더 목록 최대 높이(pt). 넘으면 스크롤. 창이 낮으면 남은 높이만큼으로 줄인다.
const LOOK_MENU_LIST_H: f32 = 320.0;
/// 창이 아주 낮아도 목록에 남기는 높이(pt).
const LOOK_MENU_LIST_MIN_H: f32 = 80.0;
/// egui Separator 기본 높이(pt).
const LOOK_MENU_SEP_H: f32 = 6.0;
/// 목록 아래 고정 줄 말고 남겨 둘 여백(팝업 프레임 아래 여백·테두리 + 여유, pt).
const LOOK_MENU_TAIL_PAD: f32 = 16.0;
/// 툴바 버튼의 파일 이름 최대 글자 수(넘으면 13자 + …).
const LOOK_BTN_STEM: usize = 14;
/// 메뉴의 파일 이름 최대 글자 수(넘으면 23자 + …).
const LOOK_MENU_STEM: usize = 24;
/// `\`를 누르는 동안의 LUT 버튼 글자. 「원본」은 ORIG·「PREVIEW · 원본 없음」과 헷갈려 쓰지 않는다.
/// prop 폰트라 백슬래시를 넣지 않는다(윈도우 맑은 고딕은 ₩로 그린다).
const LOOK_COMPARE: &str = "LUT · OFF";
/// LUT 버튼 글자와 삼각형 사이, 삼각형 폭(pt).
const LOOK_TRI_GAP: f32 = 5.0;
const LOOK_TRI_W: f32 = 7.0;
/// 툴바 로고 크기와 그 뒤 여백.
const TB_LOGO: f32 = 22.0;
const TB_LOGO_GAP: f32 = 6.0;
/// 두 묶음 사이에 남겨야 하는 최소 여백(pt). 테스트는 4 이상을 본다.
const TB_MIN_GAP: f32 = 8.0;
/// 상태 바 왼쪽 통계 줄과 오른쪽 묶음 사이 최소 여백(pt).
const STATUS_MIN_GAP: f32 = 8.0;
/// Tight 단계의 툴바 위젯 사이 간격(pt). 다른 단계는 egui 기본(8).
const TB_TIGHT_SPACING: f32 = 4.0;
/// 구도 프리셋 전부(가장 넓은 이름을 재는 데 쓴다).
const GUIDE_PRESETS: [GuidePreset; 5] = [
    GuidePreset::Off,
    GuidePreset::Thirds,
    GuidePreset::CenterCross,
    GuidePreset::Square,
    GuidePreset::Custom,
];

/// 툴바 밀도 단계. 넓은 창 → 좁은 창 순.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum ToolbarTier {
    /// 「Fit (Space)」처럼 키 힌트까지.
    #[default]
    Full,
    /// 모든 「(키)」 힌트를 라벨에서 떼어 툴팁으로(라벨 「Fit」, 툴팁 「Fit (Space)」).
    NoHints,
    /// 힌트 없음 + 구도 버튼은 「구도」 고정, LUT 버튼은 「LUT」만(이름은 툴팁, 켜짐은 채움).
    Compact,
    /// Compact + 위젯 사이 간격을 [`TB_TIGHT_SPACING`]으로. 글꼴이 넓은 윈도우(맑은 고딕·Yu Gothic)나
    /// 화면 배율에서 Compact도 최소 창(900pt)에 안 들어갈 때.
    Tight,
}

/// 단계 계산용 가장 넓은 LUT 글자 캐시([`RawBlowApp::refresh_look_worst`]).
pub(super) struct LookWorst {
    /// (언어, LUT 폴더 목록 세대, 룩 경로).
    key: (Lang, u64, String),
    text: String,
}

/// 실제로 그린 툴바(테스트가 겹침을 본다).
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct ToolbarLayout {
    pub tier: ToolbarTier,
    /// 왼쪽 묶음 위젯들의 가장 오른쪽 끝.
    pub left_end: f32,
    /// 오른쪽 묶음 위젯들의 가장 왼쪽 끝.
    pub right_start: f32,
    /// LUT 버튼 자리(단일 보기만).
    pub look: Option<Rect>,
}

/// 툴바 버튼이 하는 일.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TbAction {
    OpenFolder,
    Single,
    Grid,
    Prev,
    Next,
    Fit,
    OneToOne,
    Orig,
    Full,
    Exif,
    Hist,
    Guides,
    Transfer,
    Jump,
    Bulk,
    Settings,
    Help,
}

/// 툴바 위젯 하나. 폭 측정(tb_extent)과 그리기(ui_toolbar_item)가 같은 목록을 쓴다.
enum TbItem {
    Logo,
    Sep,
    /// toggle_btn. 힌트를 뗀 단계에서는 원래 라벨이 툴팁으로 간다.
    Toggle { label: String, tip: Option<String>, action: TbAction },
    /// 테두리 없는 ◀ ▶.
    Arrow { glyph: &'static str, action: TbAction },
    /// 위치 카운터(mono 12).
    Counter(String),
    Look(LookBtn),
    /// 강조색 전송 버튼.
    Accent { label: String, tip: Option<String>, action: TbAction },
}

/// 툴바 내용 중 바뀌는 부분. 그리기는 지금 값, 단계 측정은 최악 값을 넣는다.
struct TbContent {
    lang: Lang,
    view: ViewMode,
    counter: String,
    /// 구도 버튼 이름(번역됨). 끔이면 「구도」.
    guide: &'static str,
    look: LookBtn,
}

/// 「이름 (키)」. Full이면 그대로, 아니면 라벨은 이름만이고 원래 라벨은 툴팁으로 간다.
/// `extra`는 원래 있던 툴팁(「T 키로 전환」 등) — 함께 보인다.
fn hinted(tier: ToolbarTier, name: &str, key: &str, extra: Option<&str>) -> (String, Option<String>) {
    let full = format!("{name} ({key})");
    if tier == ToolbarTier::Full {
        (full, extra.map(str::to_string))
    } else {
        let tip = match extra {
            Some(e) => format!("{full}\n{e}"),
            None => full,
        };
        (name.to_string(), Some(tip))
    }
}

/// 툴바 위젯 목록(왼쪽, 오른쪽). 오른쪽은 right_to_left라 먼저 넣은 것이 오른쪽 끝이다.
fn toolbar_items(c: &TbContent, tier: ToolbarTier) -> (Vec<TbItem>, Vec<TbItem>) {
    let lang = c.lang;
    let toggle = |name: &str, key: &str, extra: Option<&str>, action: TbAction| {
        let (label, tip) = hinted(tier, name, key, extra);
        TbItem::Toggle { label, tip, action }
    };
    // "(T)"만으로는 토글임이 드러나지 않아 툴팁으로 보완(#72).
    let switch = tr(lang, "T 키로 전환");
    let mut left = vec![
        TbItem::Logo,
        TbItem::Sep,
        toggle(tr(lang, "폴더 열기"), &format!("{}O", cmd_key()), None, TbAction::OpenFolder),
        TbItem::Sep,
        toggle("Single", "T", Some(switch), TbAction::Single),
        toggle("Grid", "T", Some(switch), TbAction::Grid),
        TbItem::Sep,
        TbItem::Arrow { glyph: "◀", action: TbAction::Prev },
        TbItem::Counter(c.counter.clone()),
        TbItem::Arrow { glyph: "▶", action: TbAction::Next },
        TbItem::Sep,
        toggle("Fit", "Space", None, TbAction::Fit),
        toggle("1:1", "Space", None, TbAction::OneToOne),
        toggle("ORIG", "D", None, TbAction::Orig),
        toggle("Full", "F11", None, TbAction::Full),
        TbItem::Sep,
        toggle("EXIF", "I", None, TbAction::Exif),
        toggle("Hist", "H", None, TbAction::Hist),
    ];
    // 구도 보조선(#90)과 룩(#125)은 단일 보기 툴바에만. 그리드 썸네일에는 그리지 않는다.
    if c.view == ViewMode::Single {
        let full = format!("{} (])", c.guide);
        let (label, tip) = match tier {
            ToolbarTier::Full => (full, None),
            ToolbarTier::NoHints => (c.guide.to_string(), Some(full)),
            ToolbarTier::Compact | ToolbarTier::Tight => (tr(lang, "구도").to_string(), Some(full)),
        };
        left.push(TbItem::Toggle { label, tip, action: TbAction::Guides });
        left.push(TbItem::Look(c.look.clone()));
    }
    let (send, send_tip) = hinted(tier, tr(lang, "전송"), &format!("{}E", cmd_key()), None);
    let mut right = vec![
        TbItem::Accent { label: format!(" {send} "), tip: send_tip, action: TbAction::Transfer },
        toggle(tr(lang, "점프"), "G", None, TbAction::Jump),
    ];
    if c.view == ViewMode::Grid {
        right.push(toggle(tr(lang, "일괄"), "B", None, TbAction::Bulk));
    }
    // 아이콘만으로는 기능이 드러나지 않아 툴팁으로 보완(#72).
    right.push(TbItem::Toggle { label: "⚙".into(), tip: Some(tr(lang, "설정").into()), action: TbAction::Settings });
    right.push(TbItem::Toggle { label: "?".into(), tip: Some(tr(lang, "단축키").into()), action: TbAction::Help });
    (left, right)
}

/// 버튼·라벨과 같은 방식으로 글자를 배치한다(줄바꿈 없음, 기본 글꼴은 Button).
fn tb_galley(ui: &egui::Ui, text: egui::RichText) -> std::sync::Arc<egui::Galley> {
    egui::WidgetText::from(text).into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, egui::TextStyle::Button)
}

fn counter_label(pos: usize, len: usize) -> String {
    format!("{pos:03} / {len}")
}

fn counter_text(text: &str) -> egui::RichText {
    egui::RichText::new(text).font(mono(12.0)).color(theme::INK).background_color(theme::BG2)
}

fn accent_text(label: &str) -> egui::RichText {
    egui::RichText::new(label).color(Color32::from_rgb(0x0a, 0x14, 0x20))
}

/// 이 단계의 툴바 위젯 사이 간격.
fn tb_spacing(ui: &egui::Ui, tier: ToolbarTier) -> f32 {
    if tier == ToolbarTier::Tight {
        TB_TIGHT_SPACING
    } else {
        ui.style().spacing.item_spacing.x
    }
}

/// 묶음 하나가 차지하는 폭. egui 가로 배치를 그대로 따라간다: 위젯 뒤마다 `spacing`,
/// add_space는 그 자리에서 더한다. 끝은 마지막 위젯의 바깥쪽 끝이다.
fn tb_extent(ui: &egui::Ui, items: &[TbItem], spacing: f32) -> f32 {
    let pad = ui.spacing().button_padding.x;
    let (mut x, mut end) = (0.0_f32, 0.0_f32);
    for item in items {
        let (before, w, after) = match item {
            TbItem::Logo => (0.0, TB_LOGO, TB_LOGO_GAP),
            TbItem::Sep => (VSEP_GAP, 1.0, VSEP_GAP),
            TbItem::Toggle { label, .. } => (0.0, tb_galley(ui, toggle_btn_text(label, false)).size().x + 2.0 * pad, 0.0),
            TbItem::Arrow { glyph, .. } => (0.0, tb_galley(ui, egui::RichText::new(*glyph)).size().x, 0.0),
            TbItem::Counter(text) => (0.0, tb_galley(ui, counter_text(text)).size().x, 0.0),
            TbItem::Look(btn) => (0.0, look_btn_size(ui, btn).x, 0.0),
            TbItem::Accent { label, .. } => (0.0, tb_galley(ui, accent_text(label)).size().x + 2.0 * pad, 0.0),
        };
        x += before;
        end = x + w;
        x = end + spacing + after;
    }
    end
}

/// LUT 버튼 상태. 켜짐 판정은 resolve_look과 같다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LookState {
    Off,
    On,
    /// 룩이 켜져 있고 `\`를 누르고 있다.
    Compare,
}

/// LUT 버튼 모양.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct LookBtn {
    /// 화면 글자.
    pub text: String,
    pub state: LookState,
    /// 폭을 정하는 글자들. 가장 넓은 것에 맞춰 `\`를 눌러도 툴바가 흔들리지 않는다.
    pub widths: Vec<String>,
    /// 툴팁 첫 줄(Compact에서 룩 이름이 글자에서 빠졌을 때).
    pub tip_head: Option<String>,
}

/// 메뉴에서 고른 것.
enum LookPick {
    /// 빌트인 이름 또는 빈 이름(없음).
    Name(String),
    File(PathBuf),
    Settings,
}

/// 켜진 룩의 표시 이름. resolve_look 규칙과 같다: name=="file"이고 경로가 있으면 파일 stem,
/// 빌트인 이름이면 그 이름. 그 밖(빈 이름·모르는 이름)은 꺼짐(None).
pub(super) fn look_name(look: &rawblow_core::lut::LookPref) -> Option<String> {
    if look.name == "file" && !look.cube_path.is_empty() {
        Some(short_stem(Path::new(&look.cube_path), LOOK_BTN_STEM))
    } else if rawblow_core::lut::BUILTINS.contains(&look.name.as_str()) {
        Some(look.name.clone())
    } else {
        None
    }
}

/// 켜진 LUT 버튼 글자. 「LUT」·「OFF」는 FIT·ORIG 같은 약어라 번역하지 않는다.
fn look_on_label(name: &str) -> String {
    format!("LUT · {name}")
}

/// 지금 룩·비교 상태·밀도 단계에 맞는 LUT 버튼.
pub(super) fn look_button(look: &rawblow_core::lut::LookPref, compare: bool, tier: ToolbarTier) -> LookBtn {
    let Some(name) = look_name(look) else {
        return LookBtn { text: "LUT".into(), state: LookState::Off, widths: vec!["LUT".into()], tip_head: None };
    };
    let state = if compare { LookState::Compare } else { LookState::On };
    let on = look_on_label(&name);
    if tier >= ToolbarTier::Compact {
        // 비교 중에도 툴팁은 룩 이름 — 꺼져 보인다는 표시는 ACCENT 글자가 한다.
        return LookBtn { text: "LUT".into(), state, widths: vec!["LUT".into()], tip_head: Some(on) };
    }
    let text = if compare { LOOK_COMPARE.to_string() } else { on.clone() };
    LookBtn { text, state, widths: vec![on, LOOK_COMPARE.to_string()], tip_head: None }
}

/// LUT 버튼 글자(toggle_btn과 같은 글꼴).
fn look_btn_text(text: &str, color: Color32) -> egui::RichText {
    egui::RichText::new(text).font(prop(12.0)).color(color)
}

/// LUT 버튼 크기: 버튼 안쪽 여백 + 가장 넓은 글자 + 삼각형. 높이는 egui Button과 같은 규칙.
fn look_btn_size(ui: &egui::Ui, btn: &LookBtn) -> Vec2 {
    let pad = ui.spacing().button_padding;
    let text = btn
        .widths
        .iter()
        .chain(std::iter::once(&btn.text))
        .map(|t| tb_galley(ui, look_btn_text(t, theme::INK)).size())
        .fold(Vec2::ZERO, |a, b| a.max(b));
    Vec2::new(
        pad.x * 2.0 + text.x + LOOK_TRI_GAP + LOOK_TRI_W,
        (text.y + pad.y * 2.0).max(ui.spacing().interact_size.y),
    )
}

/// LUT 버튼을 그린다. 꺼짐은 투명(꺼진 토글), 켜짐은 BG3+LINE2 채움, 비교 중은 채움 없이 ACCENT 글자.
/// ▾ 글리프는 폰트에 따라 두부(□)가 되므로 삼각형을 직접 그린다.
fn paint_look_button(ui: &egui::Ui, rect: Rect, resp: &egui::Response, btn: &LookBtn) {
    let visuals = ui.style().interact(resp);
    let (fill, stroke, color) = match btn.state {
        LookState::On => (theme::BG3, theme::LINE2, theme::INK),
        LookState::Off => (Color32::TRANSPARENT, Color32::TRANSPARENT, theme::INK2),
        LookState::Compare => (Color32::TRANSPARENT, Color32::TRANSPARENT, theme::ACCENT),
    };
    let p = ui.painter();
    p.rect(rect.expand(visuals.expansion), visuals.rounding, fill, Stroke::new(1.0_f32, stroke));
    let pad = ui.spacing().button_padding.x;
    let galley = tb_galley(ui, look_btn_text(&btn.text, color));
    let pos = Pos2::new(rect.left() + pad, rect.center().y - galley.size().y / 2.0);
    p.galley(pos, galley, color);
    let c = Pos2::new(rect.right() - pad - LOOK_TRI_W / 2.0, rect.center().y + 0.5);
    let h = LOOK_TRI_W / 2.0;
    p.add(egui::Shape::convex_polygon(
        vec![c + Vec2::new(-h, -2.0), c + Vec2::new(h, -2.0), c + Vec2::new(0.0, 2.5)],
        color,
        Stroke::NONE,
    ));
}

/// 같은 `.cube`인지. LUT 폴더는 NFC 글자로 저장하고(설정 「찾아보기…」) 옛 「LUT」 버튼은 디스크 그대로
/// (맥 NFD)를 저장했으므로, 바이트가 아니라 NFC로 묶은 경로끼리 비교한다.
fn same_cube(a: &Path, b: &Path) -> bool {
    a == b || nfc_path_label(a) == nfc_path_label(b)
}

/// `.cube` 경로의 확장자 뗀 이름(NFC). `max`자를 넘으면 `max−1`자에 「…」를 붙인다.
pub(super) fn short_stem(path: &Path, max: usize) -> String {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned());
    let stem = nfc_hangul(&stem);
    if stem.chars().count() > max {
        let mut s: String = stem.chars().take(max.saturating_sub(1)).collect();
        s.push('…');
        s
    } else {
        stem
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rawblow_core::color::OutputKind;

    const KINDS: [OutputKind; 4] = [OutputKind::Srgb, OutputKind::SrgbFallback, OutputKind::DisplayP3, OutputKind::MonitorIcc];

    // ── 상태 바 보기 문구 · HUD 꼬리 ─────────────────────────────

    #[test]
    fn status_view_text_table() {
        let out = |lang, kind| match (lang, kind) {
            (_, OutputKind::Srgb) => "sRGB",
            (_, OutputKind::DisplayP3) => "Display P3",
            (Lang::Ko, OutputKind::SrgbFallback) => "sRGB(폴백)",
            (Lang::En, OutputKind::SrgbFallback) => "sRGB (fallback)",
            (Lang::Ja, OutputKind::SrgbFallback) => "sRGB（フォールバック）",
            (Lang::Ko, OutputKind::MonitorIcc) => "모니터 ICC",
            (Lang::En, OutputKind::MonitorIcc) => "Monitor ICC",
            (Lang::Ja, OutputKind::MonitorIcc) => "モニター ICC",
        };
        // (썸네일 대체, ORIG, 원본 폴백) → MODE. 약어는 번역하지 않는다.
        let modes = [
            ((false, false, false), "FIT"),
            ((false, false, true), "FIT"),
            ((false, true, false), "ORIG"),
            ((false, true, true), "PREVIEW"),
            ((true, false, false), "THUMB"),
            ((true, true, false), "THUMB"),
            ((true, true, true), "THUMB"),
        ];
        for lang in [Lang::Ko, Lang::En, Lang::Ja] {
            for kind in KINDS {
                for ((thumb, raw, fallback), mode) in modes {
                    assert_eq!(
                        status_view_text(lang, thumb, raw, fallback, kind),
                        format!("{mode} · {}", out(lang, kind)),
                        "{lang:?} {kind:?} {thumb} {raw} {fallback}"
                    );
                }
            }
        }
    }

    #[test]
    fn hud_tail_keeps_only_warnings() {
        for lang in [Lang::Ko, Lang::En, Lang::Ja] {
            assert_eq!(single_hud_warning(lang, false, false, false), None);
            assert_eq!(single_hud_warning(lang, false, true, false), None, "ORIG도 꼬리 없음");
            assert_eq!(single_hud_warning(lang, false, false, true), None, "프리뷰로 보면 폴백 기록은 무시");
            assert_eq!(single_hud_warning(lang, true, true, true), Some(tr(lang, "썸네일 · 본 이미지 열기 실패")));
            assert_eq!(single_hud_warning(lang, false, true, true), Some(tr(lang, "PREVIEW · 원본 없음")));
        }
    }

    /// 실제 앱 상태(설정 + OS 조사 결과)에서 상태 바 문구와 설정 「현재 출력」이 같은 출력 이름을 쓴다
    /// (모니터 이름을 모를 때). 윈도우 probe는 끈 상태에서도 ICC를 주므로 그 경우를 꼭 넣는다.
    #[test]
    fn status_bar_and_settings_row_name_the_output_from_app_state() {
        use rawblow_core::color::OsColor;
        RawBlowApp::settings_qa_begin();
        let mut app = RawBlowApp::for_settings_qa();
        let icc = || OsColor::Windows { icc: Some(vec![1, 2, 3]), acm_or_hdr: false };
        let cases = [
            (false, icc(), "sRGB"),
            (false, OsColor::MacOs { layer_ok: true }, "sRGB"),
            (true, OsColor::Linux, "sRGB(폴백)"),
            (true, OsColor::MacOs { layer_ok: false }, "sRGB(폴백)"),
            (true, OsColor::MacOs { layer_ok: true }, "Display P3"),
            (true, OsColor::Windows { icc: None, acm_or_hdr: false }, "sRGB(폴백)"),
            (true, OsColor::Windows { icc: Some(vec![1, 2, 3]), acm_or_hdr: true }, "sRGB(폴백)"),
            (true, icc(), "모니터 ICC"),
        ];
        for (wide, os, tail) in cases {
            app.cfg.wide_gamut = wide;
            app.os_color = os.clone();
            app.monitor_name.clear();
            let text = status_view_text(Lang::Ko, false, false, false, app.output_kind());
            assert_eq!(text, format!("FIT · {tail}"), "{wide} {os:?}");
            assert_eq!(app.wide_gamut_status(), format!("현재 출력: {tail}"), "{wide} {os:?}");
        }
    }

    // ── 테스트 도우미 ───────────────────────────────────────────

    type Texts = Vec<(String, Rect)>;

    fn walk(shapes: &[egui::Shape], out: &mut Texts) {
        for s in shapes {
            match s {
                egui::Shape::Text(t) => {
                    out.push((t.galley.text().to_string(), t.galley.rect.translate(t.pos.to_vec2())));
                }
                egui::Shape::Vec(v) => walk(v, out),
                _ => {}
            }
        }
    }

    fn texts_of(out: &egui::FullOutput) -> Texts {
        let shapes: Vec<_> = out.shapes.iter().map(|c| c.shape.clone()).collect();
        let mut texts = Vec::new();
        walk(&shapes, &mut texts);
        texts
    }

    fn joined(texts: &Texts) -> String {
        texts.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>().join("\n")
    }

    fn find<'a>(texts: &'a Texts, text: &str) -> Vec<&'a Rect> {
        texts.iter().filter(|(t, _)| t == text).map(|(_, r)| r).collect()
    }

    fn lang_ctx(lang: Lang) -> egui::Context {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        crate::fonts::install(&ctx, lang);
        ctx
    }

    fn ko_ctx() -> egui::Context {
        lang_ctx(Lang::Ko)
    }

    const SCREEN: Vec2 = Vec2::new(1280.0, 800.0);

    fn run_at<R>(
        ctx: &egui::Context,
        app: &mut RawBlowApp,
        size: Vec2,
        events: Vec<egui::Event>,
        mut f: impl FnMut(&mut RawBlowApp, &egui::Context) -> R,
    ) -> (Texts, R) {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
            events,
            ..Default::default()
        };
        let mut r = None;
        let out = ctx.run(input, |ctx| r = Some(f(app, ctx)));
        (texts_of(&out), r.expect("프레임이 한 번 돌아야 한다"))
    }

    fn frame(
        ctx: &egui::Context,
        app: &mut RawBlowApp,
        events: Vec<egui::Event>,
        f: impl FnMut(&mut RawBlowApp, &egui::Context),
    ) -> Texts {
        run_at(ctx, app, SCREEN, events, f).0
    }

    fn shell(app: &mut RawBlowApp, ctx: &egui::Context) {
        app.ui_shell(ctx);
    }

    fn screen(app: &mut RawBlowApp, ctx: &egui::Context) {
        app.ui_screen(ctx);
    }

    fn button(pos: Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE }
    }

    fn click_at(pos: Pos2) -> Vec<egui::Event> {
        vec![egui::Event::PointerMoved(pos), button(pos, true), button(pos, false)]
    }

    fn key(key: egui::Key) -> egui::Event {
        egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }
    }

    /// 단일 보기 앱. 항목 n개, 설정·지도 끔.
    fn hud_app(n: usize) -> RawBlowApp {
        RawBlowApp::settings_qa_begin();
        let mut app = crate::app::tests::filter_app(n);
        app.view = ViewMode::Single;
        app.show_settings = false;
        app.show_map = false;
        app.folder = Some(PathBuf::from("Z:/filter"));
        app
    }

    /// 0번 항목에 가짜 썸네일 텍스처를 넣는다(사진 클릭·팬이 살아 있게).
    fn give_texture(ctx: &egui::Context, app: &mut RawBlowApp) {
        let img = egui::ColorImage::new([2000, 1500], Color32::GRAY);
        let tex = ctx.load_texture("fake_photo", img, egui::TextureOptions::LINEAR);
        app.thumbs.insert(0, tex, false);
    }

    /// ui_shell의 중앙(사진) 영역.
    fn central_area() -> Rect {
        Rect::from_min_max(Pos2::new(RAIL_W, TOOLBAR_H), Pos2::new(SCREEN.x, SCREEN.y - FILMSTRIP_H - STATUS_H))
    }

    fn menu_open(ctx: &egui::Context) -> bool {
        ctx.memory(|m| m.is_popup_open(egui::Id::new(LOOK_MENU_ID)))
    }

    fn open_menu(ctx: &egui::Context) {
        ctx.memory_mut(|m| m.open_popup(egui::Id::new(LOOK_MENU_ID)));
    }

    fn look(name: &str, cube: &str) -> rawblow_core::lut::LookPref {
        rawblow_core::lut::LookPref { name: name.into(), cube_path: cube.into() }
    }

    /// 툴바 띠 안의 LUT 버튼 글자(「LUT」 또는 「LUT · …」) 사각형.
    fn lut_text(texts: &Texts) -> Option<Rect> {
        texts.iter().find(|(t, r)| t.starts_with("LUT") && r.bottom() <= TOOLBAR_H).map(|(_, r)| *r)
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rb_views_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    const GOOD_CUBE: &str = "LUT_3D_SIZE 2\n1 0 0\n1 0 0\n1 0 0\n1 0 0\n1 0 0\n1 0 0\n1 0 0\n1 0 0\n";

    /// 메뉴를 열고(버튼 클릭) 메뉴가 그려진 프레임의 글자를 돌려준다.
    fn open_menu_by_click(ctx: &egui::Context, app: &mut RawBlowApp) -> Texts {
        let texts = frame(ctx, app, vec![], shell);
        let c = lut_text(&texts).expect("툴바 LUT 버튼").center();
        frame(ctx, app, click_at(c), shell);
        assert!(menu_open(ctx), "LUT 버튼을 누르면 메뉴가 열린다");
        frame(ctx, app, vec![], shell);
        frame(ctx, app, vec![], shell)
    }

    // ── 룩 줄 → 툴바 LUT 버튼 ───────────────────────────────────

    #[test]
    fn look_bar_is_gone_and_lut_button_is_on_the_single_toolbar_only() {
        let ctx = ko_ctx();
        let mut app = hud_app(3);
        let texts = frame(&ctx, &mut app, vec![], shell);
        assert!(egui::containers::panel::PanelState::load(&ctx, egui::Id::new("look")).is_none(), "룩 줄 없음");
        assert!(lut_text(&texts).is_some(), "단일 보기 툴바에 LUT 버튼");
        let (_, layout) = run_at(&ctx, &mut app, SCREEN, vec![], |app, ctx| app.ui_toolbar(ctx));
        let look_rect = layout.look.expect("LUT 버튼 자리");
        // 구도 버튼 바로 뒤(왼쪽 묶음 맨 끝).
        assert!((look_rect.right() - layout.left_end).abs() < 0.5);

        app.view = ViewMode::Grid;
        let grid = frame(&ctx, &mut app, vec![], shell);
        assert!(!joined(&grid).contains("LUT"), "그리드에는 LUT 버튼이 없다");
        let (_, layout) = run_at(&ctx, &mut app, SCREEN, vec![], |app, ctx| app.ui_toolbar(ctx));
        assert!(layout.look.is_none());

        app.view = ViewMode::Single;
        app.fullscreen = true;
        let ctx = ko_ctx();
        give_texture(&ctx, &mut app);
        frame(&ctx, &mut app, vec![], screen);
        let full = frame(&ctx, &mut app, vec![], screen);
        assert!(!joined(&full).contains("LUT"), "전체화면에는 LUT UI가 없다");
        assert!(egui::containers::panel::PanelState::load(&ctx, egui::Id::new("look")).is_none());
        assert!(!find(&full, "FULLSCREEN · ESC").is_empty(), "전체화면 꼬리는 그대로");
    }

    #[test]
    fn look_button_labels() {
        let full = ToolbarTier::Full;
        let b = |l: rawblow_core::lut::LookPref, compare| look_button(&l, compare, full);
        assert_eq!((b(look("", ""), false).text, b(look("", ""), false).state), ("LUT".to_string(), LookState::Off));
        assert_eq!((b(look("Warm", ""), false).text, b(look("Warm", ""), false).state), ("LUT · Warm".to_string(), LookState::On));
        let long = look("file", "/Users/me/LUTs/kodak_2383_film_emulation.cube");
        assert_eq!(b(long.clone(), false).text, "LUT · kodak_2383_fi…", "14자 넘으면 13자 + …");
        assert_eq!(b(look("file", "/l/abcdefghijklmn.cube"), false).text, "LUT · abcdefghijklmn", "14자는 그대로");
        // NFD 한글 파일 이름도 NFC로 센다.
        let nfd = "\u{1112}\u{1161}\u{11AB}\u{1100}\u{1173}\u{11AF}";
        assert_eq!(b(look("file", &format!("/l/{nfd}.cube")), false).text, "LUT · 한글");
        // 비교 중: 「LUT · OFF」, 폭은 켜짐·비교 라벨 중 넓은 쪽.
        let cmp = b(look("Warm", ""), true);
        assert_eq!((cmp.text.as_str(), cmp.state), ("LUT · OFF", LookState::Compare));
        assert_eq!(cmp.widths, vec!["LUT · Warm".to_string(), "LUT · OFF".to_string()]);
        assert_eq!(b(look("Warm", ""), false).widths, cmp.widths, "켜짐·비교가 같은 폭 후보");
        // 모르는 이름·경로 없는 file·빈 이름은 꺼짐(비교 키를 눌러도).
        for off in [look("Foo", ""), look("file", ""), look("", "/l/a.cube")] {
            for compare in [false, true] {
                let o = b(off.clone(), compare);
                assert_eq!((o.text.as_str(), o.state), ("LUT", LookState::Off), "{off:?}");
            }
        }
        // Compact: 글자는 「LUT」뿐, 이름은 툴팁 첫 줄, 켜짐은 채움(상태로 그린다).
        let c = look_button(&look("Tungsten", ""), false, ToolbarTier::Compact);
        assert_eq!((c.text.as_str(), c.state, c.tip_head.as_deref()), ("LUT", LookState::On, Some("LUT · Tungsten")));
        let c = look_button(&look("Tungsten", ""), true, ToolbarTier::Compact);
        assert_eq!((c.text.as_str(), c.state, c.tip_head.as_deref()), ("LUT", LookState::Compare, Some("LUT · Tungsten")));
        // 백슬래시는 prop 글자에 넣지 않는다(윈도우 맑은 고딕은 ₩).
        assert!(!LOOK_COMPARE.contains('\\'));
    }

    #[test]
    fn lut_button_width_is_stable_while_comparing() {
        let ctx = ko_ctx();
        let mut app = hud_app(3);
        for pref in [look("Warm", ""), look("Pan", ""), look("file", "/l/kodak_2383_film_emulation.cube")] {
            app.cfg.look = pref;
            app.look_compare = false;
            let (_, on) = run_at(&ctx, &mut app, SCREEN, vec![], |app, ctx| app.ui_toolbar(ctx));
            app.look_compare = true;
            let (texts, cmp) = run_at(&ctx, &mut app, SCREEN, vec![], |app, ctx| app.ui_toolbar(ctx));
            assert_eq!(on.tier, cmp.tier);
            assert_eq!(on.look, cmp.look, "{:?}", app.cfg.look);
            assert_eq!(on.right_start, cmp.right_start);
            let off = lut_text(&texts).expect("비교 글자");
            assert!(on.look.unwrap().contains_rect(off.shrink(0.5)), "비교 글자가 버튼 안");
        }
        // 꺼짐은 「LUT」 폭이라 켜짐보다 좁다.
        app.look_compare = false;
        app.cfg.look = look("", "");
        let (_, off) = run_at(&ctx, &mut app, SCREEN, vec![], |app, ctx| app.ui_toolbar(ctx));
        app.cfg.look = look("Pan", "");
        let (_, on) = run_at(&ctx, &mut app, SCREEN, vec![], |app, ctx| app.ui_toolbar(ctx));
        assert!(off.look.unwrap().width() < on.look.unwrap().width());
    }

    // ── 툴바 밀도 ──────────────────────────────────────────────

    /// 최악 내용: 장수 n장, LUT 폴더에 긴 영문·한글 이름, 지금 룩은 가장 긴 파일, 룩 켜짐.
    fn worst_app_n(lang: Lang, n: usize) -> RawBlowApp {
        let mut app = hud_app(n);
        app.lang = lang;
        let long_en = PathBuf::from("/Users/me/LUTs/Kodak_Portra_400_Film_Emulation_v2.cube");
        let long_ko = PathBuf::from("/Users/me/LUTs/코닥포트라필름에뮬레이션따뜻한톤.cube");
        app.lut_files = vec![long_en, long_ko.clone()];
        app.lut_dir_status = rawblow_core::lut::CubeDir::Found { shown: 2, total: 2 };
        app.cfg.look = look("file", &long_ko.display().to_string());
        app
    }

    fn worst_app(lang: Lang) -> RawBlowApp {
        worst_app_n(lang, 1500)
    }

    /// 모든 글꼴을 `scale`배로 넓힌 문맥. 윈도우 글꼴(맑은 고딕·Yu Gothic Medium)은 맥 글꼴보다
    /// 툴바 글자가 약 8% 넓다 — 테스트가 이 기계의 OS 글꼴에만 기대지 않게 10%로 흉내 낸다.
    fn wide_ctx(lang: Lang, scale: f32) -> egui::Context {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        let mut defs = crate::fonts::definitions(lang);
        for fd in defs.font_data.values_mut() {
            fd.tweak.scale *= scale;
        }
        ctx.set_fonts(defs);
        ctx
    }

    /// 창 폭·언어·보기·구도·비교·장수 전부에서 두 묶음이 겹치지 않는다. 돌려주는 값은 본 단계들.
    fn assert_toolbar_never_overlaps(ctx_of: impl Fn(Lang) -> egui::Context, what: &str) -> Vec<(Lang, ToolbarTier)> {
        let mut seen = Vec::new();
        for lang in [Lang::Ko, Lang::En, Lang::Ja] {
            let ctx = ctx_of(lang);
            for n in [3, 1500, 12000] {
                let mut app = worst_app_n(lang, n);
                for w in [900.0, 1000.0, 1100.0, 1280.0, 1440.0] {
                    for view in [ViewMode::Single, ViewMode::Grid] {
                        app.view = view;
                        for guides in GUIDE_PRESETS {
                            app.cfg.guides = guides;
                            for compare in [false, true] {
                                app.look_compare = compare;
                                let (_, l) = run_at(&ctx, &mut app, Vec2::new(w, 800.0), vec![], |app, ctx| app.ui_toolbar(ctx));
                                let gap = l.right_start - l.left_end;
                                assert!(
                                    gap >= 4.0,
                                    "{what} {lang:?} {n}장 {w} {view:?} {guides:?} compare={compare}: 겹침/여백 {gap:.1} (tier {:?})",
                                    l.tier
                                );
                                if !seen.contains(&(lang, l.tier)) {
                                    seen.push((lang, l.tier));
                                }
                            }
                        }
                    }
                }
            }
        }
        seen
    }

    #[test]
    fn toolbar_never_overlaps_at_any_width_language_or_view() {
        assert_toolbar_never_overlaps(lang_ctx, "OS 글꼴");
    }

    /// 글자가 넓은 글꼴(윈도우)에서도 900pt에 들어간다 — Compact로도 모자라면 Tight가 간격을 줄인다.
    #[test]
    fn toolbar_never_overlaps_with_wide_fonts() {
        let seen = assert_toolbar_never_overlaps(|lang| wide_ctx(lang, 1.10), "넓은 글꼴");
        assert!(seen.iter().any(|(_, t)| *t == ToolbarTier::Tight), "넓은 글꼴 900pt에서는 Tight까지 내려간다 {seen:?}");
        assert!(seen.iter().any(|(_, t)| *t == ToolbarTier::Full), "넓은 창은 여전히 Full {seen:?}");
    }

    /// Tight는 Compact와 같은 글자에 간격만 줄인다. 측정과 그리기가 같다.
    #[test]
    fn tight_tier_keeps_compact_labels_and_measures_what_it_draws() {
        let ctx = wide_ctx(Lang::Ja, 1.10);
        let mut app = worst_app_n(Lang::Ja, 12000);
        let size = Vec2::new(900.0, 800.0);
        let (texts, l) = run_at(&ctx, &mut app, size, vec![], |app, ctx| app.ui_toolbar(ctx));
        assert_eq!(l.tier, ToolbarTier::Tight);
        assert!(!find(&texts, "LUT").is_empty(), "LUT 버튼은 「LUT」만");
        assert!(!find(&texts, tr(Lang::Ja, "구도")).is_empty(), "구도 버튼은 고정 이름");
        let (_, (ml, mr)) = run_at(&ctx, &mut app, size, vec![], |app, ctx| {
            let mut m = (0.0, 0.0);
            egui::CentralPanel::default().show(ctx, |ui| {
                let (left, right) = toolbar_items(&app.toolbar_content(l.tier), l.tier);
                let spacing = tb_spacing(ui, l.tier);
                assert_eq!(spacing, TB_TIGHT_SPACING);
                m = (tb_extent(ui, &left, spacing), tb_extent(ui, &right, spacing));
            });
            m
        });
        assert!((ml - (l.left_end - 12.0)).abs() < 0.5, "왼쪽 측정 {ml} 실제 {}", l.left_end - 12.0);
        assert!((mr - (size.x - 12.0 - l.right_start)).abs() < 0.5, "오른쪽 측정 {mr} 실제 {}", size.x - 12.0 - l.right_start);
    }

    #[test]
    fn toolbar_is_full_at_1440_ko_and_drops_hints_when_narrow() {
        let ctx = ko_ctx();
        let mut app = worst_app(Lang::Ko);
        for guides in GUIDE_PRESETS {
            app.cfg.guides = guides;
            let (texts, l) = run_at(&ctx, &mut app, Vec2::new(1440.0, 900.0), vec![], |app, ctx| app.ui_toolbar(ctx));
            assert_eq!(l.tier, ToolbarTier::Full, "1440 KO는 힌트까지 {guides:?}");
            assert!(!find(&texts, "Fit (Space)").is_empty(), "힌트 보임");
        }
        let (texts, l) = run_at(&ctx, &mut app, Vec2::new(900.0, 700.0), vec![], |app, ctx| app.ui_toolbar(ctx));
        assert_ne!(l.tier, ToolbarTier::Full, "900에서는 힌트를 뗀다");
        assert!(find(&texts, "Fit (Space)").is_empty());
        assert!(!find(&texts, "Fit").is_empty(), "라벨은 이름만");
    }

    #[test]
    fn toolbar_tier_is_the_same_for_single_and_grid() {
        for lang in [Lang::Ko, Lang::En, Lang::Ja] {
            let ctx = lang_ctx(lang);
            let mut app = worst_app(lang);
            let mut seen = std::collections::BTreeSet::new();
            for w in (900..=1600).step_by(20) {
                let size = Vec2::new(w as f32, 800.0);
                app.view = ViewMode::Single;
                let (_, single) = run_at(&ctx, &mut app, size, vec![], |app, ctx| app.ui_toolbar(ctx));
                app.view = ViewMode::Grid;
                let (_, grid) = run_at(&ctx, &mut app, size, vec![], |app, ctx| app.ui_toolbar(ctx));
                assert_eq!(single.tier, grid.tier, "{lang:?} {w}: T로 바꿔도 같은 단계");
                seen.insert(single.tier);
            }
            assert!(seen.contains(&ToolbarTier::Full), "{lang:?}: 넓은 창은 Full {seen:?}");
        }
    }

    /// 단계 측정(tb_extent)이 실제로 그린 폭과 같다 — 측정과 그리기가 같은 목록을 쓴다.
    #[test]
    fn toolbar_measure_matches_what_is_drawn() {
        for lang in [Lang::Ko, Lang::En, Lang::Ja] {
            let ctx = lang_ctx(lang);
            let mut app = worst_app(lang);
            for (w, view) in [(1440.0, ViewMode::Single), (1000.0, ViewMode::Single), (900.0, ViewMode::Grid), (900.0, ViewMode::Single)] {
                app.view = view;
                let size = Vec2::new(w, 800.0);
                let (_, l) = run_at(&ctx, &mut app, size, vec![], |app, ctx| app.ui_toolbar(ctx));
                let (_, (ml, mr)) = run_at(&ctx, &mut app, size, vec![], |app, ctx| {
                    let mut m = (0.0, 0.0);
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let (left, right) = toolbar_items(&app.toolbar_content(l.tier), l.tier);
                        let spacing = tb_spacing(ui, l.tier);
                        m = (tb_extent(ui, &left, spacing), tb_extent(ui, &right, spacing));
                    });
                    m
                });
                let drawn_left = l.left_end - 12.0;
                let drawn_right = (w - 12.0) - l.right_start;
                assert!((ml - drawn_left).abs() < 0.5, "{lang:?} {w} {view:?} 왼쪽 측정 {ml} 실제 {drawn_left}");
                assert!((mr - drawn_right).abs() < 0.5, "{lang:?} {w} {view:?} 오른쪽 측정 {mr} 실제 {drawn_right}");
            }
        }
    }

    #[test]
    fn hints_move_to_tooltips_and_compact_fixes_guide_and_lut_labels() {
        let tip = "T 키로 전환";
        assert_eq!(hinted(ToolbarTier::Full, "Single", "T", Some(tip)), ("Single (T)".into(), Some(tip.into())));
        assert_eq!(
            hinted(ToolbarTier::NoHints, "Single", "T", Some(tip)),
            ("Single".into(), Some(format!("Single (T)\n{tip}")))
        );
        assert_eq!(hinted(ToolbarTier::Compact, "Fit", "Space", None), ("Fit".into(), Some("Fit (Space)".into())));
        let labels = |tier, guide: &'static str| {
            let c = TbContent {
                lang: Lang::Ko,
                view: ViewMode::Single,
                counter: counter_label(1, 3),
                guide,
                look: look_button(&look("Warm", ""), false, tier),
            };
            let (left, right) = toolbar_items(&c, tier);
            left.iter()
                .chain(&right)
                .filter_map(|it| match it {
                    TbItem::Toggle { label, tip, .. } | TbItem::Accent { label, tip, .. } => Some((label.clone(), tip.clone())),
                    TbItem::Look(b) => Some((b.text.clone(), b.tip_head.clone())),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let full = labels(ToolbarTier::Full, "삼분할");
        let no = labels(ToolbarTier::NoHints, "삼분할");
        let compact = labels(ToolbarTier::Compact, "삼분할");
        let has = |v: &Vec<(String, Option<String>)>, l: &str, t: Option<&str>| {
            v.iter().any(|(a, b)| a == l && b.as_deref() == t)
        };
        assert!(has(&full, "삼분할 (])", None));
        assert!(has(&full, "LUT · Warm", None));
        assert!(has(&no, "삼분할", Some("삼분할 (])")));
        assert!(has(&no, "LUT · Warm", None));
        assert!(has(&compact, "구도", Some("삼분할 (])")), "Compact 구도는 고정 이름");
        assert!(has(&compact, "LUT", Some("LUT · Warm")), "Compact LUT는 이름을 툴팁으로");
        // NoHints부터는 어떤 라벨에도 「(키)」가 없다.
        for v in [&no, &compact] {
            for (label, _) in v.iter() {
                assert!(!label.contains(" ("), "힌트가 남았다: {label}");
            }
        }
        assert!(has(&no, "폴더 열기", Some(&format!("폴더 열기 ({}O)", cmd_key()))));
        assert!(has(&no, " 전송 ", Some(&format!("전송 ({}E)", cmd_key()))));
    }

    // ── 상태 바 문구 · HUD 꼬리(화면) ────────────────────────────

    fn status_band(texts: &Texts) -> Vec<(String, Rect)> {
        texts.iter().filter(|(_, r)| r.top() >= SCREEN.y - STATUS_H - 0.5).cloned().collect()
    }

    #[test]
    fn view_text_moves_from_hud_to_status_bar() {
        let ctx = ko_ctx();
        let mut app = hud_app(3);
        give_texture(&ctx, &mut app);
        frame(&ctx, &mut app, vec![], shell);
        let texts = frame(&ctx, &mut app, vec![], shell);
        let status = status_band(&texts);
        let fit = status.iter().find(|(t, _)| t == "FIT · sRGB").map(|(_, r)| *r).expect("상태 바에 FIT · sRGB");
        let saved = status.iter().find(|(t, _)| t == "saved").map(|(_, r)| *r).expect("saved");
        let zoom = status.iter().find(|(t, _)| t.ends_with('%')).map(|(_, r)| *r).expect("배율");
        assert!(fit.right() < zoom.left() && zoom.right() < saved.left(), "FIT · 출력 · 배율 · ● saved 순서");
        assert!(fit.left() > SCREEN.x / 2.0, "오른쪽 묶음");
        assert!(!joined(&texts).lines().any(|t| t.starts_with("FIT · ") && !status.iter().any(|(s, _)| s == t)));
        assert_eq!(texts.iter().filter(|(t, _)| t.contains("FIT")).count(), 1, "HUD에는 FIT 문구가 없다");

        // 썸네일 대체: HUD에는 경고, 상태 바에는 THUMB.
        app.decode_fails.insert(0, DECODE_DEAD_THRESHOLD);
        let texts = frame(&ctx, &mut app, vec![], shell);
        let warn = find(&texts, "썸네일 · 본 이미지 열기 실패");
        assert!(!warn.is_empty() && warn.iter().all(|r| r.bottom() < SCREEN.y / 2.0), "경고는 HUD 오른쪽 위");
        assert!(status_band(&texts).iter().any(|(t, _)| t == "THUMB · sRGB"));
        app.decode_fails.clear();

        // ORIG를 켰는데 원본을 못 구했다.
        app.full_raw = true;
        app.orig_fallback.insert(0);
        let texts = frame(&ctx, &mut app, vec![], shell);
        assert!(!find(&texts, "PREVIEW · 원본 없음").is_empty(), "경고는 HUD에 남는다");
        assert!(status_band(&texts).iter().any(|(t, _)| t == "PREVIEW · sRGB"));
        app.orig_fallback.clear();
        let texts = frame(&ctx, &mut app, vec![], shell);
        assert!(status_band(&texts).iter().any(|(t, _)| t == "ORIG · sRGB"));
        assert!(find(&texts, "PREVIEW · 원본 없음").is_empty());

        // 그리드 상태 바에는 문구가 없다.
        app.view = ViewMode::Grid;
        let texts = frame(&ctx, &mut app, vec![], shell);
        assert!(!joined(&texts).contains(" · sRGB"), "그리드에는 보기 문구 없음");
    }

    /// 최소 창(900pt)에서도 상태 바 오른쪽 묶음(보기 문구 · 배율 · 저장)이 왼쪽 통계에 닿지 않는다.
    /// 가장 긴 경우: 장수 5자리(라벨이 고루 나뉘어 P/H/X/미분류가 모두 4자리), 유휴 뒤의 긴 프레임 시간,
    /// PREVIEW + 모니터 ICC, 「저장 실패」. 모자라면 왼쪽 프레임 통계 줄만 줄임표로 잘린다.
    /// 흔한 경우(장수 4자리, 보통 프레임 시간)에는 자르지 않는다.
    #[test]
    fn status_bar_view_text_fits_at_min_width() {
        let labels = [Label::Pick, Label::Hold, Label::Reject, Label::Unrated];
        for lang in [Lang::Ko, Lang::En, Lang::Ja] {
            let ctx = lang_ctx(lang);
            for (n, ms) in [(1500, 16.7), (12000, 16.7), (12000, 1234.5), (12000, 98765.4)] {
                let mut app = hud_app(n);
                for (i, it) in app.items.iter_mut().enumerate() {
                    it.entry.label = labels[i % 4];
                }
                app.index = n - 1;
                app.lang = lang;
                app.frame_ms = ms;
                app.cfg.wide_gamut = true;
                app.os_color = rawblow_core::color::OsColor::Windows { icc: Some(vec![1]), acm_or_hdr: false };
                app.save_error = Some("x".into());
                app.full_raw = true;
                app.orig_fallback.insert(n - 1);
                let (texts, full_w) = run_at(&ctx, &mut app, Vec2::new(900.0, 700.0), vec![], |app, ctx| {
                    app.ui_status_bar(ctx);
                    let fps = 1000.0 / app.frame_ms;
                    let stats = format!("· {:.1}ms · {:.0} FPS · GPU wgpu · PRELOAD ±{}", app.frame_ms, fps, app.cfg.preload);
                    ctx.fonts(|f| f.layout_no_wrap(stats, mono(10.5), theme::INK).size().x)
                });
                let stats = texts.iter().find(|(t, _)| t.contains("PRELOAD")).map(|(_, r)| *r).expect("통계");
                let (view, left) = texts
                    .iter()
                    .find(|(t, _)| t.starts_with("PREVIEW · "))
                    .map(|(t, r)| (t.clone(), r.left()))
                    .expect("보기 문구");
                let gap = left - stats.right();
                assert!(gap >= 4.0, "{lang:?} {n}장 {ms}ms: {view:?}와 통계 사이 {gap:.0}pt");
                let first = texts.iter().find(|(t, _)| t.starts_with("READY · ")).map(|(_, r)| *r).expect("READY");
                assert!(first.left() >= 12.0 - 0.5, "왼쪽 첫 줄은 그대로");
                assert!(stats.width() <= full_w + 0.5);
            }
            // 흔한 경우(장수 4자리, FIT · sRGB, saved, 보통 프레임 시간)에는 900pt에서도 통계를 자르지 않는다.
            let mut app = hud_app(1500);
            app.lang = lang;
            app.frame_ms = 16.7;
            let (texts, full_w) = run_at(&ctx, &mut app, Vec2::new(900.0, 700.0), vec![], |app, ctx| {
                app.ui_status_bar(ctx);
                let stats = format!("· {:.1}ms · {:.0} FPS · GPU wgpu · PRELOAD ±{}", app.frame_ms, 1000.0 / app.frame_ms, app.cfg.preload);
                ctx.fonts(|f| f.layout_no_wrap(stats, mono(10.5), theme::INK).size().x)
            });
            let stats = texts.iter().find(|(t, _)| t.contains("PRELOAD")).map(|(_, r)| *r).expect("통계");
            assert!((stats.width() - full_w).abs() < 0.5, "{lang:?}: 흔한 경우에는 통계를 자르지 않는다");
            assert!(texts.iter().any(|(t, _)| t == "FIT · sRGB"));
        }
    }

    // ── 치트시트 ───────────────────────────────────────────────

    /// 글자와 그 글자를 자르는 영역(clip). 스크롤 영역 밖으로 밀린 줄을 찾는 데 쓴다.
    fn clipped_texts(out: &egui::FullOutput) -> Vec<(String, Rect, Rect)> {
        let mut v = Vec::new();
        for c in &out.shapes {
            let mut texts = Vec::new();
            walk(std::slice::from_ref(&c.shape), &mut texts);
            v.extend(texts.into_iter().map(|(t, r)| (t, r, c.clip_rect)));
        }
        v
    }

    /// 치트시트(고정 크기 창) 두 열의 마지막 줄이 스크롤하지 않아도 보인다(#66 「표가 잘림」 회귀 방지).
    /// 룩 고르기 안내도 보인다.
    #[test]
    fn cheatsheet_last_rows_fit_without_scrolling() {
        for lang in [Lang::Ko, Lang::En, Lang::Ja] {
            for size in [SCREEN, Vec2::new(900.0, 600.0)] {
                let ctx = lang_ctx(lang);
                let mut app = hud_app(3);
                app.lang = lang;
                app.show_help = true;
                let mut texts = Vec::new();
                for _ in 0..3 {
                    let input = egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
                        ..Default::default()
                    };
                    let out = ctx.run(input, |ctx| app.ui_help(ctx));
                    texts = clipped_texts(&out);
                }
                for last in [tr(lang, "드래그앤드롭으로 폴더 열기"), tr(lang, "태그 해제")] {
                    let (_, r, clip) = texts
                        .iter()
                        .find(|(t, _, _)| t == last)
                        .unwrap_or_else(|| panic!("{lang:?} {size:?}: 마지막 줄 {last:?}가 그려지지 않았다"));
                    assert!(
                        r.bottom() <= clip.bottom() + 0.5,
                        "{lang:?} {size:?}: {last:?} 아래 끝 {} > 보이는 영역 {}",
                        r.bottom(),
                        clip.bottom()
                    );
                }
                let cap = tr(lang, "룩 고르기: 툴바 LUT · 폴더는 설정");
                let (_, r, clip) = texts.iter().find(|(t, _, _)| t == cap).unwrap_or_else(|| panic!("{lang:?}: 룩 고르기 안내"));
                assert!(clip.contains_rect(*r) && r.bottom() <= size.y, "{lang:?} {size:?}: 안내가 잘렸다 {r:?} {clip:?}");
                // 푸터 오른쪽: 왼쪽의 「Esc 닫기」와 겹치지 않는다.
                let close = texts.iter().find(|(t, _, _)| t == tr(lang, "닫기")).map(|(_, r, _)| *r).expect("닫기");
                assert!(r.left() - close.right() >= 8.0, "{lang:?}: 안내가 「닫기」에 닿는다 {r:?} {close:?}");
            }
        }
    }

    // ── 룩 메뉴 ────────────────────────────────────────────────

    #[test]
    fn menu_lists_lut_folder_cubes_in_order_and_rereads_on_open() {
        let dir = temp_dir("menu_list");
        for name in ["a.cube", "B.CUBE", "c.txt", ".hidden.cube"] {
            std::fs::write(dir.join(name), GOOD_CUBE).unwrap();
        }
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub").join("d.cube"), GOOD_CUBE).unwrap();
        let ctx = ko_ctx();
        let mut app = hud_app(3);
        // 폴더만 바꿔 두고 다시 읽지 않는다 — 메뉴를 열 때 읽어야 한다.
        app.cfg.lut_dir = dir.display().to_string();
        assert!(app.lut_files.is_empty());
        let texts = open_menu_by_click(&ctx, &mut app);
        assert_eq!(app.lut_files.len(), 2, "열 때 다시 읽는다");
        let top = |t: &str| find(&texts, t).first().map(|r| r.top()).unwrap_or_else(|| panic!("메뉴에 {t}: {}", joined(&texts)));
        let order = ["없음", "Warm", "Tungsten", "Chrome", "Pan", "Vivid", "Snap", "a", "B", "LUT 폴더 설정…", "누르고 있는 동안 룩 끄기"];
        for pair in order.windows(2) {
            assert!(top(pair[0]) < top(pair[1]), "{} 다음 {}", pair[0], pair[1]);
        }
        for absent in ["c", "d", ".hidden", "hidden"] {
            assert!(find(&texts, absent).is_empty(), "{absent}는 없어야 한다");
        }
        for caption in [
            "설정에서 LUT 폴더를 지정하면 .cube 파일이 여기 나옵니다",
            "LUT 폴더에 .cube 파일이 없습니다",
            "LUT 폴더를 찾을 수 없습니다",
        ] {
            assert!(find(&texts, caption).is_empty(), "목록이 있으면 캡션 없음");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn menu_captions_for_unset_empty_and_missing_folder() {
        let empty = temp_dir("menu_empty");
        std::fs::write(empty.join("readme.txt"), "x").unwrap();
        let gone = std::env::temp_dir().join(format!("rb_views_gone_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&gone);
        for (dir, caption) in [
            (String::new(), "설정에서 LUT 폴더를 지정하면 .cube 파일이 여기 나옵니다"),
            (empty.display().to_string(), "LUT 폴더에 .cube 파일이 없습니다"),
            (gone.display().to_string(), "LUT 폴더를 찾을 수 없습니다"),
        ] {
            let ctx = ko_ctx();
            let mut app = hud_app(3);
            app.cfg.lut_dir = dir;
            let texts = open_menu_by_click(&ctx, &mut app);
            assert!(!find(&texts, caption).is_empty(), "캡션 {caption}: {}", joined(&texts));
            assert!(!find(&texts, "LUT 폴더 설정…").is_empty());
        }
        let _ = std::fs::remove_dir_all(&empty);

        // 폴더는 있는데 권한이 없다: 「찾을 수 없음」이 아니다(root는 권한과 상관없이 읽어 만들 수 없다).
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let locked = temp_dir("menu_locked");
            std::fs::write(locked.join("a.cube"), GOOD_CUBE).unwrap();
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
            let blocked = std::fs::read_dir(&locked).is_err();
            let ctx = ko_ctx();
            let mut app = hud_app(3);
            app.cfg.lut_dir = locked.display().to_string();
            let texts = open_menu_by_click(&ctx, &mut app);
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
            let _ = std::fs::remove_dir_all(&locked);
            if blocked {
                assert!(!find(&texts, "LUT 폴더를 읽을 수 없습니다(접근 권한)").is_empty(), "{}", joined(&texts));
                assert!(find(&texts, "LUT 폴더를 찾을 수 없습니다").is_empty());
            }
        }
    }

    #[test]
    fn current_file_outside_the_folder_stays_in_the_menu() {
        let ctx = ko_ctx();
        let mut app = hud_app(3);
        app.cfg.look = look("file", "/somewhere/else/MyFilm.cube");
        let texts = open_menu_by_click(&ctx, &mut app);
        assert!(!find(&texts, "MyFilm").is_empty(), "지금 룩 파일이 한 줄 보인다");
        assert!(lut_text(&texts).is_some_and(|_| texts.iter().any(|(t, _)| t == "LUT · MyFilm")));
    }

    #[test]
    fn picking_from_menu_sets_look_and_same_pick_does_not_save() {
        let dir = temp_dir("menu_pick");
        let good = dir.join("good.cube");
        let bad = dir.join("bad.cube");
        std::fs::write(&good, GOOD_CUBE).unwrap();
        std::fs::write(&bad, "LUT_1D_SIZE 2\n0 0 0\n").unwrap();
        let ctx = ko_ctx();
        let mut app = hud_app(3);
        app.cfg.lut_dir = dir.display().to_string();

        // 빌트인.
        let texts = open_menu_by_click(&ctx, &mut app);
        let warm = *find(&texts, "Warm").first().expect("메뉴에 Warm");
        frame(&ctx, &mut app, click_at(warm.center()), shell);
        assert_eq!(app.cfg.look.name, "Warm");
        assert!(!menu_open(&ctx), "고르면 닫힌다");
        let persisted = RawBlowApp::settings_qa_persist_count();
        assert!(persisted > 0);
        // 같은 빌트인을 다시 골라도 저장하지 않는다.
        let texts = open_menu_by_click(&ctx, &mut app);
        assert!(texts.iter().any(|(t, _)| t == "LUT · Warm"), "버튼에 룩 이름");
        let warm = *find(&texts, "Warm").first().unwrap();
        frame(&ctx, &mut app, click_at(warm.center()), shell);
        assert_eq!(RawBlowApp::settings_qa_persist_count(), persisted);

        // 폴더의 유효한 .cube.
        let texts = open_menu_by_click(&ctx, &mut app);
        let row = *find(&texts, "good").first().expect("메뉴에 good");
        frame(&ctx, &mut app, click_at(row.center()), shell);
        assert_eq!(app.cfg.look.name, "file");
        assert_eq!(app.cfg.look.cube_path, good.display().to_string());
        let persisted = RawBlowApp::settings_qa_persist_count();
        // 같은 파일 재선택은 저장하지 않는다.
        let texts = open_menu_by_click(&ctx, &mut app);
        let row = *find(&texts, "good").first().unwrap();
        frame(&ctx, &mut app, click_at(row.center()), shell);
        assert_eq!(RawBlowApp::settings_qa_persist_count(), persisted);

        // 잘못된 .cube: 토스트, 이전 룩 유지, 저장 없음.
        app.toast = None;
        let texts = open_menu_by_click(&ctx, &mut app);
        let row = *find(&texts, "bad").first().expect("메뉴에 bad");
        frame(&ctx, &mut app, click_at(row.center()), shell);
        assert!(app.toast.is_some(), "토스트");
        assert_eq!(app.cfg.look.cube_path, good.display().to_string(), "이전 룩 유지");
        assert_eq!(RawBlowApp::settings_qa_persist_count(), persisted);

        // 없음.
        let texts = open_menu_by_click(&ctx, &mut app);
        let none = *find(&texts, "없음").first().unwrap();
        frame(&ctx, &mut app, click_at(none.center()), shell);
        assert_eq!(app.cfg.look.name, "");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn menu_folder_settings_item_opens_settings() {
        let ctx = ko_ctx();
        let mut app = hud_app(3);
        app.settings_reset_armed = true;
        let texts = open_menu_by_click(&ctx, &mut app);
        let item = *find(&texts, "LUT 폴더 설정…").first().expect("LUT 폴더 설정…");
        frame(&ctx, &mut app, click_at(item.center()), screen);
        assert!(app.show_settings, "설정 화면을 연다");
        assert!(!app.settings_reset_armed, "⚙ 클릭과 같은 부수 처리");
        assert!(app.cache_size.is_none());
        frame(&ctx, &mut app, vec![], screen);
        assert!(!menu_open(&ctx));
    }

    // ── 키·포인터 가드 ─────────────────────────────────────────

    #[test]
    fn lut_button_click_across_frames_opens_menu() {
        // 실제 마우스는 누르기와 떼기가 다른 프레임에 온다. 고아 정리(ui_screen)를 거쳐도 열려 있어야 한다.
        let ctx = ko_ctx();
        let mut app = hud_app(3);
        give_texture(&ctx, &mut app);
        let texts = frame(&ctx, &mut app, vec![], screen);
        let c = lut_text(&texts).expect("LUT 버튼").center();
        frame(&ctx, &mut app, vec![egui::Event::PointerMoved(c)], screen);
        frame(&ctx, &mut app, vec![button(c, true)], screen);
        assert!(!menu_open(&ctx), "누르기만으로는 열리지 않는다");
        frame(&ctx, &mut app, vec![button(c, false)], screen);
        frame(&ctx, &mut app, vec![], screen);
        assert!(menu_open(&ctx), "떼면 열리고 다음 프레임에도 열려 있다");
        assert!(app.fit);
        // 다시 누르면 닫힌다.
        frame(&ctx, &mut app, click_at(c), screen);
        assert!(!menu_open(&ctx));
    }

    #[test]
    fn shield_blocks_photo_click_and_closes_menu() {
        let center = central_area().center();
        // 대조: 메뉴가 닫혀 있으면 사진 클릭이 1:1을 요청한다.
        let ctx = ko_ctx();
        let mut app = hud_app(3);
        give_texture(&ctx, &mut app);
        frame(&ctx, &mut app, vec![], shell);
        frame(&ctx, &mut app, click_at(center), shell);
        assert!(!app.fit, "대조: 메뉴 없이 누르면 1:1");

        let ctx = ko_ctx();
        let mut app = hud_app(3);
        give_texture(&ctx, &mut app);
        frame(&ctx, &mut app, vec![], shell);
        open_menu(&ctx);
        frame(&ctx, &mut app, vec![], shell);
        frame(&ctx, &mut app, click_at(center), shell);
        assert!(!menu_open(&ctx), "사진을 누르면 메뉴가 닫힌다");
        assert!(app.fit, "닫는 클릭은 Fit↔1:1에 쓰지 않는다");
        assert!(!app.one_to_one_pending);
        // 닫힌 뒤에는 사진 클릭이 다시 산다.
        frame(&ctx, &mut app, vec![], shell);
        frame(&ctx, &mut app, click_at(center), shell);
        assert!(!app.fit);

        // 디코딩 실패 화면의 「클릭하여 재시도」로도 새지 않는다.
        let ctx = ko_ctx();
        let mut app = hud_app(3);
        app.decode_fails.insert(0, DECODE_DEAD_THRESHOLD);
        frame(&ctx, &mut app, vec![], shell);
        open_menu(&ctx);
        frame(&ctx, &mut app, vec![], shell);
        frame(&ctx, &mut app, click_at(center), shell);
        assert!(!menu_open(&ctx));
        assert_eq!(app.decode_fails.get(&0), Some(&DECODE_DEAD_THRESHOLD), "재시도가 불리지 않았다");
    }

    #[test]
    fn shield_lets_drag_pan_the_photo() {
        let ctx = ko_ctx();
        let mut app = hud_app(3);
        give_texture(&ctx, &mut app);
        frame(&ctx, &mut app, vec![], shell);
        app.fit = false;
        app.zoom = 1.0;
        frame(&ctx, &mut app, vec![], shell);
        open_menu(&ctx);
        frame(&ctx, &mut app, vec![], shell);
        let pan0 = app.pan;
        let start = central_area().center();
        frame(&ctx, &mut app, vec![egui::Event::PointerMoved(start), button(start, true)], shell);
        for dx in [20.0, 40.0, 60.0] {
            frame(&ctx, &mut app, vec![egui::Event::PointerMoved(start - Vec2::new(dx, 0.0))], shell);
        }
        frame(&ctx, &mut app, vec![button(start - Vec2::new(60.0, 0.0), false)], shell);
        assert!((app.pan.x - pan0.x).abs() > 30.0, "방패는 click 전용 — 드래그는 사진 팬 {pan0:?} → {:?}", app.pan);
    }

    #[test]
    fn wheel_does_not_flip_while_menu_open() {
        let wheel = || egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: Vec2::new(0.0, -10.0),
            modifiers: egui::Modifiers::NONE,
        };
        let ctx = ko_ctx();
        let mut app = hud_app(3);
        open_menu(&ctx);
        frame(&ctx, &mut app, vec![wheel()], |app, ctx| app.handle_keys(ctx));
        assert_eq!(app.index, 0, "메뉴가 열려 있으면 휠로 넘기지 않는다");
        ctx.memory_mut(|m| m.close_popup());
        frame(&ctx, &mut app, vec![wheel()], |app, ctx| app.handle_keys(ctx));
        assert_eq!(app.index, 1, "대조: 메뉴가 닫혀 있으면 넘긴다");
    }

    /// update와 같은 순서(키 처리 → 화면 분기).
    fn keys_then_screen(app: &mut RawBlowApp, ctx: &egui::Context) {
        app.handle_keys(ctx);
        app.ui_screen(ctx);
    }

    /// 단일 보기에서 메뉴가 열린 채 Esc: 메뉴만 닫히고 보기·위치·배율은 그대로.
    /// F11: 전체화면에는 LUT 버튼이 없어 같은 프레임에 메뉴가 닫히고, 다음 Esc는 전체화면을 푼다.
    #[test]
    fn escape_closes_only_the_menu_and_f11_drops_it() {
        let ctx = ko_ctx();
        let mut app = hud_app(3);
        give_texture(&ctx, &mut app);
        let texts = frame(&ctx, &mut app, vec![], keys_then_screen);
        let c = lut_text(&texts).expect("LUT 버튼").center();
        frame(&ctx, &mut app, click_at(c), keys_then_screen);
        frame(&ctx, &mut app, vec![], keys_then_screen);
        assert!(menu_open(&ctx));
        frame(&ctx, &mut app, vec![key(egui::Key::Escape)], keys_then_screen);
        assert!(!menu_open(&ctx), "Esc는 메뉴를 닫는다");
        assert_eq!((app.fullscreen, app.view, app.index, app.fit), (false, ViewMode::Single, 0, true), "그 밖은 그대로");

        frame(&ctx, &mut app, click_at(c), keys_then_screen);
        frame(&ctx, &mut app, vec![], keys_then_screen);
        assert!(menu_open(&ctx));
        frame(&ctx, &mut app, vec![key(egui::Key::F11)], keys_then_screen);
        assert!(app.fullscreen);
        assert!(!menu_open(&ctx), "전체화면으로 가는 프레임에 메뉴가 닫힌다");
        frame(&ctx, &mut app, vec![key(egui::Key::Escape)], keys_then_screen);
        assert!(!app.fullscreen, "다음 Esc는 전체화면을 푼다");
        frame(&ctx, &mut app, vec![], keys_then_screen);
        assert!(!menu_open(&ctx), "돌아와도 메뉴가 다시 뜨지 않는다");
    }

    /// 메뉴 안의 스크롤바·안내 줄을 눌러도 메뉴는 열린 채(항목을 누르면 닫힌다 — picking 테스트).
    #[test]
    fn clicking_inside_the_menu_off_items_keeps_it_open() {
        let dir = temp_dir("menu_scroll");
        for i in 0..40 {
            std::fs::write(dir.join(format!("file_{i:02}.cube")), GOOD_CUBE).unwrap();
        }
        let ctx = ko_ctx();
        let mut app = hud_app(3);
        app.cfg.lut_dir = dir.display().to_string();
        let texts = open_menu_by_click(&ctx, &mut app);
        let area = ctx.memory(|m| m.area_rect(egui::Id::new(LOOK_MENU_ID))).expect("메뉴 자리");
        let row = |texts: &Texts, t: &str| find(texts, t).first().map(|r| **r);
        let first = row(&texts, "file_00").expect("file_00");
        let mid = row(&texts, "file_08").expect("file_08");
        // 오른쪽 끝 스크롤바 트랙.
        let bar = Pos2::new(area.right() - 7.0, mid.center().y);
        let texts = frame(&ctx, &mut app, vec![egui::Event::PointerMoved(bar)], shell);
        let texts2 = frame(&ctx, &mut app, click_at(bar), shell);
        let _ = texts;
        assert!(menu_open(&ctx), "스크롤바를 눌러도 열린 채");
        let texts = frame(&ctx, &mut app, vec![], shell);
        let moved = row(&texts, "file_00").is_none_or(|r| (r.top() - first.top()).abs() > 1.0)
            || row(&texts2, "file_00").is_none_or(|r| (r.top() - first.top()).abs() > 1.0);
        assert!(moved, "스크롤바를 누른 것이 맞다(목록이 넘어감)");
        // 안내 줄(선택 항목이 아님).
        let hint = *find(&texts, "누르고 있는 동안 룩 끄기").first().expect("안내 줄");
        frame(&ctx, &mut app, click_at(hint.center()), shell);
        frame(&ctx, &mut app, vec![], shell);
        assert!(menu_open(&ctx), "안내 줄을 눌러도 열린 채");
        assert_eq!(app.cfg.look.name, "", "룩은 그대로");
        // 대조: 메뉴 밖(사진)을 누르면 닫힌다.
        frame(&ctx, &mut app, click_at(central_area().center()), shell);
        assert!(!menu_open(&ctx));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 메뉴를 열 때 폴더를 다시 읽어 더 긴 이름이 생겨도, 열린 동안은 툴바 단계·LUT 버튼 자리가 그대로다.
    /// 닫힌 뒤에 반영된다.
    #[test]
    fn toolbar_does_not_jump_while_menu_is_open() {
        let dir = temp_dir("menu_freeze");
        std::fs::write(dir.join("readme.txt"), "x").unwrap();
        let ctx = lang_ctx(Lang::En);
        let mut app = hud_app(3);
        app.lang = Lang::En;
        app.cfg.lut_dir = dir.display().to_string();
        app.reload_lut_files();
        let toolbar = |app: &mut RawBlowApp, ctx: &egui::Context| app.ui_toolbar(ctx);
        let (_, before) = run_at(&ctx, &mut app, SCREEN, vec![], toolbar);
        for name in ["Kodak_Portra_400_Film_Emulation_v2.cube", "코닥포트라필름에뮬레이션따뜻한톤.cube"] {
            std::fs::write(dir.join(name), GOOD_CUBE).unwrap();
        }
        let c = before.look.expect("LUT 버튼").center();
        run_at(&ctx, &mut app, SCREEN, click_at(c), toolbar);
        assert_eq!(app.lut_files.len(), 2, "열 때 다시 읽었다");
        for _ in 0..3 {
            let (_, open) = run_at(&ctx, &mut app, SCREEN, vec![], toolbar);
            assert!(menu_open(&ctx));
            assert_eq!((open.tier, open.look), (before.tier, before.look), "열린 동안 그대로");
        }
        run_at(&ctx, &mut app, SCREEN, click_at(c), toolbar);
        assert!(!menu_open(&ctx));
        let (_, after) = run_at(&ctx, &mut app, SCREEN, vec![], toolbar);
        assert_ne!(after.tier, before.tier, "닫힌 뒤에는 긴 이름을 반영한다(이 테스트가 뜻이 있는지)");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 폴더 밖 파일 룩에서 빌트인으로 바꿔도 툴바 단계가 그대로다(그 파일 줄은 메뉴에 남는다).
    #[test]
    fn picking_a_builtin_does_not_change_the_toolbar_tier() {
        let ctx = ko_ctx();
        let mut app = hud_app(3);
        app.cfg.look = look("file", "/somewhere/else/코닥포트라필름에뮬레이션따뜻한톤.cube");
        for w in (900..=1500).step_by(10) {
            let size = Vec2::new(w as f32, 800.0);
            app.cfg.look.name = "file".into();
            let (_, file) = run_at(&ctx, &mut app, size, vec![], |app, ctx| app.ui_toolbar(ctx));
            app.cfg.look.name = "Tungsten".into();
            let (_, builtin) = run_at(&ctx, &mut app, size, vec![], |app, ctx| app.ui_toolbar(ctx));
            app.cfg.look.name = String::new();
            let (_, off) = run_at(&ctx, &mut app, size, vec![], |app, ctx| app.ui_toolbar(ctx));
            assert_eq!((file.tier, builtin.tier, off.tier), (file.tier, file.tier, file.tier), "{w}");
        }
        // 짧은 이름의 파일로 바꿔도(경로가 바뀌어도) 세션 동안 가장 넓은 이름을 붙잡아 둔다.
        // 처음부터 짧은 파일이었던 앱과는 단계가 갈리는 폭이 있어야 이 검사가 뜻이 있다.
        let long_look = look("file", "/somewhere/else/코닥포트라필름에뮬레이션따뜻한톤.cube");
        let short_look = look("file", "/l/a.cube");
        let toolbar = |app: &mut RawBlowApp, ctx: &egui::Context| app.ui_toolbar(ctx);
        let mut differs = 0;
        for w in (900..=1500).step_by(5) {
            let size = Vec2::new(w as f32, 800.0);
            let mut seen = hud_app(3);
            seen.cfg.look = long_look.clone();
            let (_, long) = run_at(&ctx, &mut seen, size, vec![], toolbar);
            seen.cfg.look = short_look.clone();
            let (_, after) = run_at(&ctx, &mut seen, size, vec![], toolbar);
            assert_eq!(after.tier, long.tier, "{w}: 짧은 파일로 바꿔도 단계 그대로");
            let mut fresh = hud_app(3);
            fresh.cfg.look = short_look.clone();
            let (_, short) = run_at(&ctx, &mut fresh, size, vec![], toolbar);
            differs += usize::from(short.tier != long.tier);
        }
        assert!(differs > 0, "긴 이름 때문에 단계가 달라지는 폭이 있어야 한다");
    }

    /// 「LUT 폴더 설정…」으로 열면 「LUT 폴더」 행이 보이게 스크롤한다. ⚙로 열면 맨 위 그대로.
    #[test]
    fn menu_settings_item_scrolls_to_the_lut_folder_row() {
        let ctx = ko_ctx();
        let mut app = hud_app(3);
        let texts = open_menu_by_click(&ctx, &mut app);
        let item = *find(&texts, "LUT 폴더 설정…").first().expect("LUT 폴더 설정…");
        frame(&ctx, &mut app, click_at(item.center()), screen);
        assert!(app.show_settings && app.settings_focus_lut);
        let mut texts = Vec::new();
        for _ in 0..3 {
            texts = frame(&ctx, &mut app, vec![], screen);
        }
        assert!(!app.settings_focus_lut, "한 번만");
        let row = find(&texts, "LUT 폴더");
        assert!(
            row.first().is_some_and(|r| r.top() >= 52.0 && r.bottom() <= SCREEN.y),
            "「LUT 폴더」 행이 화면 안에 보인다: {row:?}"
        );

        // 대조: ⚙(open_settings)로 열면 스크롤하지 않는다 — 그 행은 첫 화면 밖이다.
        let ctx = ko_ctx();
        let mut app = hud_app(3);
        app.open_settings();
        let mut texts = Vec::new();
        for _ in 0..3 {
            texts = frame(&ctx, &mut app, vec![], screen);
        }
        assert!(find(&texts, "LUT 폴더").is_empty(), "⚙로 열면 맨 위(이 테스트가 뜻이 있는지)");
    }

    /// 맥에서 만든 NFD 이름 폴더를 NFC로 적어 둔 LUT 폴더와, 옛 「LUT」 버튼이 저장한 NFD 경로가
    /// 같은 파일로 보인다: 줄은 하나, 다시 골라도 저장하지 않는다.
    #[test]
    fn nfd_and_nfc_paths_to_the_same_cube_are_one_row() {
        let root = temp_dir("menu_nfd");
        let nfd_dir = root.join("\u{1105}\u{116E}\u{11A8}"); // 「룩」 NFD
        std::fs::create_dir_all(&nfd_dir).unwrap();
        std::fs::write(nfd_dir.join("a.cube"), GOOD_CUBE).unwrap();
        let ctx = ko_ctx();
        let mut app = hud_app(3);
        app.cfg.lut_dir = nfc_path_label(&nfd_dir);
        app.cfg.look = look("file", &nfd_dir.join("a.cube").display().to_string());
        let texts = open_menu_by_click(&ctx, &mut app);
        assert_eq!(find(&texts, "a").len(), 1, "같은 파일은 한 줄: {}", joined(&texts));
        let saved = RawBlowApp::settings_qa_persist_count();
        let row = *find(&texts, "a").first().unwrap();
        frame(&ctx, &mut app, click_at(row.center()), shell);
        assert_eq!(RawBlowApp::settings_qa_persist_count(), saved, "같은 파일을 다시 골라도 저장하지 않는다");
        assert_eq!(app.cfg.look.name, "file");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 폴더 밖 파일 룩에서 빌트인으로 바꿔도 그 파일 줄이 메뉴에 남아 다시 고를 수 있다.
    #[test]
    fn outside_file_stays_reachable_after_picking_a_builtin() {
        let dir = temp_dir("menu_outside");
        let film = dir.join("MyFilm.cube");
        std::fs::write(&film, GOOD_CUBE).unwrap();
        let ctx = ko_ctx();
        let mut app = hud_app(3);
        app.cfg.look = look("file", &film.display().to_string());
        let texts = open_menu_by_click(&ctx, &mut app);
        let warm = *find(&texts, "Warm").first().unwrap();
        frame(&ctx, &mut app, click_at(warm.center()), shell);
        assert_eq!(app.cfg.look.name, "Warm");
        let texts = open_menu_by_click(&ctx, &mut app);
        let row = *find(&texts, "MyFilm").first().expect("빌트인으로 바꿔도 그 파일 줄이 남는다");
        frame(&ctx, &mut app, click_at(row.center()), shell);
        assert_eq!(app.cfg.look, look("file", &film.display().to_string()), "다시 고르면 파일 룩");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 낮은 창(최소 900x600)에서 목록이 길고 폴더 밖 줄까지 있어도 메뉴가 화면 안에 있고 LUT 버튼을 덮지 않는다.
    #[test]
    fn long_menu_fits_below_the_button_on_a_short_window() {
        let dir = temp_dir("menu_tall");
        for i in 0..40 {
            std::fs::write(dir.join(format!("file_{i:02}.cube")), GOOD_CUBE).unwrap();
        }
        for (size, outside) in [
            (Vec2::new(900.0, 600.0), true),
            (Vec2::new(900.0, 600.0), false),
            (SCREEN, true),
            (Vec2::new(1440.0, 900.0), false),
        ] {
            let ctx = ko_ctx();
            let mut app = hud_app(3);
            app.cfg.lut_dir = dir.display().to_string();
            app.reload_lut_files();
            if outside {
                app.cfg.look = look("file", "/somewhere/else/MyFilm.cube");
            }
            let toolbar = |app: &mut RawBlowApp, ctx: &egui::Context| app.ui_toolbar(ctx);
            let (_, l) = run_at(&ctx, &mut app, size, vec![], toolbar);
            let button = l.look.expect("LUT 버튼");
            run_at(&ctx, &mut app, size, click_at(button.center()), toolbar);
            for _ in 0..3 {
                run_at(&ctx, &mut app, size, vec![], toolbar);
            }
            assert!(menu_open(&ctx));
            let area = ctx.memory(|m| m.area_rect(egui::Id::new(LOOK_MENU_ID))).expect("메뉴 자리");
            assert!(area.top() >= button.bottom() - 0.5, "{size:?} {outside}: 메뉴가 버튼을 덮는다 {area:?} {button:?}");
            assert!(area.bottom() <= size.y + 0.5, "{size:?} {outside}: 메뉴가 화면 밖 {area:?}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn orphan_menu_closes_and_does_not_come_back() {
        type Leave = fn(&mut RawBlowApp);
        type Back = fn(&mut RawBlowApp);
        let cases: [(&str, Leave, Back); 4] = [
            ("T(그리드)", |a| a.view = ViewMode::Grid, |a| a.view = ViewMode::Single),
            ("F11(전체화면)", |a| a.fullscreen = true, |a| a.fullscreen = false),
            ("설정", |a| a.open_settings(), |a| a.show_settings = false),
            ("모달(치트시트)", |a| a.show_help = true, |a| a.show_help = false),
        ];
        for (what, leave, back) in cases {
            let ctx = ko_ctx();
            let mut app = hud_app(3);
            frame(&ctx, &mut app, vec![], screen);
            open_menu(&ctx);
            frame(&ctx, &mut app, vec![], screen);
            assert!(menu_open(&ctx), "{what}: 단일 보기에서는 열린 채로 남는다");
            leave(&mut app);
            frame(&ctx, &mut app, vec![], screen);
            assert!(!menu_open(&ctx), "{what}: 떠나면 닫힌다");
            back(&mut app);
            frame(&ctx, &mut app, vec![], screen);
            let texts = frame(&ctx, &mut app, vec![], screen);
            assert!(!menu_open(&ctx), "{what}: 돌아와도 다시 뜨지 않는다");
            assert!(!joined(&texts).contains("Tungsten"), "{what}: 메뉴 글자 없음");
        }
    }
}
