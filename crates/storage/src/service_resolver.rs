use crate::Storage;
use anyhow::Result;
use overture_core::{
    LivePresentationItem, MediaKind, PresentationSlide, ServiceItem, ServiceItemKind, ThemeConfig,
};

impl Storage {
    /// Resolves a playlist [`ServiceItem`] into a complete [`LivePresentationItem`] ready for `StateEngine::dispatch`.
    pub async fn resolve_service_item(
        &self,
        item: &ServiceItem,
        fallback_theme: &ThemeConfig,
    ) -> Result<LivePresentationItem> {
        let theme = if let Some(tid) = item.params.theme_id {
            self.list_themes()
                .await?
                .into_iter()
                .find(|t| t.id == tid)
                .map(|t| t.config)
                .unwrap_or_else(|| fallback_theme.clone())
        } else if item.kind == ServiceItemKind::Bible {
            ThemeConfig::scripture_preset()
        } else {
            fallback_theme.clone()
        };

        let auto_dur = item.params.auto_advance_secs;

        match item.kind {
            ServiceItemKind::Song => {
                if let Some(song_id) = item.ref_id
                    && let Some(song) = self.get_song(song_id).await?
                {
                    let mut live = LivePresentationItem::from_song(&song, Some(item.id), theme);
                    if let Some(d) = auto_dur {
                        for s in &mut live.slides {
                            if s.duration_secs.is_none() {
                                s.duration_secs = Some(d);
                            }
                        }
                    }
                    return Ok(live);
                }
                Ok(empty_live_item(item, theme))
            }
            ServiceItemKind::Bible => {
                let ver_id = item.params.bible_version_id.unwrap_or(1);
                let versions = self.list_bible_versions().await?;
                let abbr = versions
                    .iter()
                    .find(|v| v.id == ver_id)
                    .map(|v| v.abbr.as_str())
                    .unwrap_or("");

                let slides = if !item.params.custom_slides.is_empty() {
                    item.params
                        .custom_slides
                        .iter()
                        .enumerate()
                        .map(|(idx, txt)| PresentationSlide {
                            index: idx,
                            text: txt.clone(),
                            footer: Some(item.title.clone()),
                            forced_break: false,
                            duration_secs: auto_dur,
                        })
                        .collect()
                } else {
                    let verses = self.search_bible(ver_id, &item.title, 50).await?;
                    verses
                        .into_iter()
                        .enumerate()
                        .map(|(idx, v)| PresentationSlide {
                            index: idx,
                            footer: Some(v.reference(Some(abbr))),
                            text: v.text,
                            forced_break: false,
                            duration_secs: auto_dur,
                        })
                        .collect()
                };

                Ok(LivePresentationItem {
                    service_item_id: Some(item.id),
                    song_id: None,
                    title: item.title.clone(),
                    subtitle: item.subtitle.clone(),
                    notes: item.params.notes.clone().unwrap_or_default(),
                    kind: ServiceItemKind::Bible,
                    slides,
                    theme,
                    media_path: None,
                    media_kind: None,
                })
            }
            ServiceItemKind::Media => {
                let mut media_path = None;
                let mut media_kind = None;
                if let Some(mid) = item.ref_id
                    && let Some(m) = self
                        .list_media(true)
                        .await?
                        .into_iter()
                        .find(|m| m.id == mid)
                {
                    media_path = Some(m.path);
                    media_kind = Some(m.kind);
                }
                let caption = match media_kind {
                    Some(MediaKind::Audio) => format!("♪ {}", item.title),
                    _ => String::new(),
                };
                Ok(LivePresentationItem {
                    service_item_id: Some(item.id),
                    song_id: None,
                    title: item.title.clone(),
                    subtitle: item.subtitle.clone(),
                    notes: item.params.notes.clone().unwrap_or_default(),
                    kind: ServiceItemKind::Media,
                    slides: vec![PresentationSlide {
                        index: 0,
                        text: caption,
                        footer: None,
                        forced_break: false,
                        duration_secs: auto_dur,
                    }],
                    theme,
                    media_path,
                    media_kind,
                })
            }
            ServiceItemKind::Announcement | ServiceItemKind::Presentation => {
                let raw_slides = if item.params.custom_slides.is_empty() {
                    vec![item.title.clone()]
                } else {
                    item.params.custom_slides.clone()
                };
                let slides = raw_slides
                    .into_iter()
                    .enumerate()
                    .map(|(idx, text)| PresentationSlide {
                        index: idx,
                        text,
                        footer: item.params.custom_title.clone(),
                        forced_break: false,
                        duration_secs: auto_dur,
                    })
                    .collect();

                Ok(LivePresentationItem {
                    service_item_id: Some(item.id),
                    song_id: None,
                    title: item.title.clone(),
                    subtitle: item.subtitle.clone(),
                    notes: item.params.notes.clone().unwrap_or_default(),
                    kind: item.kind,
                    slides,
                    theme,
                    media_path: None,
                    media_kind: None,
                })
            }
            ServiceItemKind::Countdown => {
                let label = item
                    .params
                    .countdown_label
                    .clone()
                    .unwrap_or_else(|| item.title.clone());
                Ok(LivePresentationItem {
                    service_item_id: Some(item.id),
                    song_id: None,
                    title: item.title.clone(),
                    subtitle: "Contagem Regressiva".to_string(),
                    notes: item.params.notes.clone().unwrap_or_default(),
                    kind: ServiceItemKind::Countdown,
                    slides: vec![PresentationSlide {
                        index: 0,
                        text: String::new(),
                        footer: Some(label),
                        forced_break: false,
                        duration_secs: None,
                    }],
                    theme,
                    media_path: None,
                    media_kind: None,
                })
            }
        }
    }
}

fn empty_live_item(item: &ServiceItem, theme: ThemeConfig) -> LivePresentationItem {
    LivePresentationItem {
        service_item_id: Some(item.id),
        song_id: None,
        title: item.title.clone(),
        subtitle: item.subtitle.clone(),
        notes: String::new(),
        kind: item.kind,
        slides: vec![PresentationSlide {
            index: 0,
            text: item.title.clone(),
            footer: None,
            forced_break: false,
            duration_secs: None,
        }],
        theme,
        media_path: None,
        media_kind: None,
    }
}
