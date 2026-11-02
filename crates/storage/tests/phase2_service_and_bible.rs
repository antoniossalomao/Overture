//! Phase 2 Acceptance Test: Assemble a 10-item worship service (songs, Bible readings,
//! announcements, media, countdown) + temporary playlist, resolve items, and navigate slides.

use overture_core::{
    CoreCommand, MediaItem, MediaKind, Service, ServiceItem, ServiceItemKind, ServiceItemParams,
    Song, SongSlide, StateEngine, ThemeConfig,
};
use overture_storage::{SongInsertOutcome, Storage};

#[tokio::test]
async fn phase2_assembles_10_item_service_and_navigates_slides() -> anyhow::Result<()> {
    let store = Storage::connect_memory().await?;
    let engine = StateEngine::new();

    // Insert 5 songs
    let mut song_ids = Vec::new();
    for i in 1..=5 {
        let mut s = Song::new_draft(
            format!("Cântico Congregacional {i}"),
            "Hinário Público",
            "public_domain",
        );
        s.slides = vec![
            SongSlide {
                id: 0,
                song_id: 0,
                order: 0,
                text: format!("Estrofe 1 do cântico {i}\nLouvai ao Senhor"),
                forced_break: false,
                duration_secs: None,
            },
            SongSlide {
                id: 0,
                song_id: 0,
                order: 1,
                text: format!("Estrofe 2 do cântico {i}\nPara sempre amém"),
                forced_break: false,
                duration_secs: None,
            },
        ];
        if let SongInsertOutcome::Inserted { id, .. } = store.insert_song(&s, false).await? {
            song_ids.push(id);
        }
    }

    let media_id = store
        .upsert_media(&MediaItem {
            id: 0,
            name: "Fundo Abertura".to_string(),
            path: "motion://aurora".to_string(),
            kind: MediaKind::Video,
            duration_secs: Some(120.0),
            archived: false,
        })
        .await?;

    let svc = store
        .create_service("2026-11-08", "Culto Solene da Noite")
        .await?;

    // Build exactly 10 heterogeneous worship items
    let mut draft_items = Vec::new();
    // 1. Countdown
    draft_items.push(ServiceItem {
        id: 0,
        service_id: svc.id,
        order: 0,
        kind: ServiceItemKind::Countdown,
        ref_id: None,
        title: "Boas-vindas".to_string(),
        subtitle: "5 minutos".to_string(),
        params: ServiceItemParams {
            countdown_seconds: Some(300),
            countdown_label: Some("O culto começa em instantes".to_string()),
            ..Default::default()
        },
    });
    // 2..=6: 5 Songs
    for (idx, &sid) in song_ids.iter().enumerate() {
        draft_items.push(ServiceItem {
            id: 0,
            service_id: svc.id,
            order: (idx + 1) as i32,
            kind: ServiceItemKind::Song,
            ref_id: Some(sid),
            title: format!("Cântico Congregacional {}", idx + 1),
            subtitle: "Hinário Público".to_string(),
            params: ServiceItemParams::default(),
        });
    }
    // 7. Bible Reading 1
    draft_items.push(ServiceItem {
        id: 0,
        service_id: svc.id,
        order: 6,
        kind: ServiceItemKind::Bible,
        ref_id: None,
        title: "Salmos 23:1-4".to_string(),
        subtitle: "TB1917".to_string(),
        params: ServiceItemParams {
            bible_version_id: Some(1),
            ..Default::default()
        },
    });
    // 8. Announcement
    draft_items.push(ServiceItem {
        id: 0,
        service_id: svc.id,
        order: 7,
        kind: ServiceItemKind::Announcement,
        ref_id: None,
        title: "Avisos da Semana".to_string(),
        subtitle: "Secretaria".to_string(),
        params: ServiceItemParams {
            custom_slides: vec![
                "Quarta-feira às 19h30:\nCulto de Oração e Estudo Bíblico".to_string(),
                "Sábado às 18h00:\nEnsaio Geral do Ministério de Louvor".to_string(),
            ],
            ..Default::default()
        },
    });
    // 9. Media Video
    draft_items.push(ServiceItem {
        id: 0,
        service_id: svc.id,
        order: 8,
        kind: ServiceItemKind::Media,
        ref_id: Some(media_id),
        title: "Vídeo Institucional".to_string(),
        subtitle: "Vídeo".to_string(),
        params: ServiceItemParams::default(),
    });
    // 10. Closing Bible Blessing
    draft_items.push(ServiceItem {
        id: 0,
        service_id: svc.id,
        order: 9,
        kind: ServiceItemKind::Bible,
        ref_id: None,
        title: "João 3:16".to_string(),
        subtitle: "TB1917".to_string(),
        params: ServiceItemParams {
            bible_version_id: Some(1),
            ..Default::default()
        },
    });

    assert_eq!(draft_items.len(), 10);
    let saved_items = store.replace_service_items(svc.id, &draft_items).await?;
    assert_eq!(saved_items.len(), 10);

    // Navigate across every single item & slide in the 10-item service
    let default_theme = ThemeConfig::default();
    let mut total_projected_slides = 0usize;
    for item in &saved_items {
        let live = store.resolve_service_item(item, &default_theme).await?;
        let slide_count = live.slides.len();
        assert!(slide_count >= 1);

        let st = engine.dispatch(CoreCommand::PresentItem {
            item: Box::new(live),
            start_slide: 0,
        });
        assert_eq!(st.current_slide_index, 0);
        total_projected_slides += 1;

        for expected_idx in 1..slide_count {
            let st_next = engine.dispatch(CoreCommand::NextSlide);
            assert_eq!(st_next.current_slide_index, expected_idx);
            total_projected_slides += 1;
        }
    }
    // 1 (countdown) + 5*2 (10 song slides) + 4 (Ps 23:1-4) + 2 (announcements) + 1 (media) + 1 (John 3:16) = 19 slides
    assert_eq!(total_projected_slides, 19);

    // Temporary playlist check (in-memory only, not persisted in SQLite `services`)
    let temp_playlist = Service::new_temporary("Lista Rápida de Ensaio");
    assert!(temp_playlist.is_temporary);
    assert_eq!(store.list_services(true).await?.len(), 1);

    Ok(())
}
