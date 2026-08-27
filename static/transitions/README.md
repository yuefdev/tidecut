# Grafik geçiş kareleri

Her klasör, bir grafik geçişin 24 karelik WebP dizisidir (640×360, `f01.webp`–`f24.webp`).
`src/lib/editor/transition-fx.ts` bu kareleri önizleme sırasında yükler:

- `screen` modu (light-leak, film-burn, glitch): kareler ekrana *screen* blend ile bindirilir.
- `mask` modu (ink, brush): karelerin parlaklığı alfa kanalına çevrilir ve klip
  `destination-in/out` ile maskelenir (mürekkep/fırça şekli klibi açar/kapatır).

## Kaynaklar (Pixabay Content License — ücretsiz, ticari kullanım serbest, atıf gerekmez)

| Klasör      | Kaynak video |
| ----------- | ------------ |
| light-leak  | https://pixabay.com/videos/light-leak-film-burn-transition-211191/ |
| film-burn   | https://pixabay.com/videos/film-burn-film-damage-burn-332995/ |
| glitch      | https://pixabay.com/videos/glitch-transition-effect-overlay-169178/ |
| ink         | https://pixabay.com/videos/ink-splash-transition-243025/ |
| brush       | https://pixabay.com/videos/transition-brush-black-white-23325/ |

## Yeniden üretme

Kaynak mp4'ten 24 kare (SÜRE = videonun saniye cinsinden uzunluğu):

```sh
ffmpeg -i kaynak.mp4 \
  -vf "fps=$(awk "BEGIN{print 24/SÜRE}"),scale=640:360:force_original_aspect_ratio=increase,crop=640:360" \
  -frames:v 24 -c:v libwebp -q:v 72 -f image2 "hedef/f%02d.webp"
```

Yeni bir geçiş eklemek için: klasörü buraya koy, `transition-fx.ts` içindeki
`TRANSITION_FX` listesine ve `timeline-engine.ts` `TransitionType`'a ekle
(maske tabanlıysa `MASK_TRANSITION_TYPES`'a da), `transition-presets.ts`
kataloğuna etiket/önizleme bilgisi ver ve `timeline-adapter.ts`'te export
karşılığını (`dissolve`) işaretle.
