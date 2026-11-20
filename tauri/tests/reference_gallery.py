"""Assemble an offline gallery from this installed application's actual screenshots."""
import base64
import html
from pathlib import Path


def write_reference_gallery(output, revision, webview):
    screens = [
        ('Сводка за полный период', 'reference-data-01-complete-current-and-base.png'),
        ('Частичные данные', 'reference-data-03-partial-adjacent-year.png'),
        ('Пустой интервал', 'reference-data-04-empty-not-zero.png'),
        ('Анализ изменений', 'reference-data-02-unequal-interval-changes.png'),
        ('Календарь', 'reference-reports-10-shared-day-draft.png'),
        ('Динамика', 'reference-reports-08-analytics-chartPanel.png'),
        ('Состав работ', 'reference-reports-08-analytics-structurePanel.png'),
        ('Сравнение', 'reference-reports-08-analytics-comparison.png'),
        ('Отклонения', 'reference-reports-08-analytics-usualPanel.png'),
        ('Частота', 'reference-reports-08-analytics-regularityPanel.png'),
        ('Отчёты и импорт Excel', 'reference-reports-07-import.png'),
        ('Группы работ', 'reference-reports-04-menu-groups.png'),
        ('Настройки с увеличенным шрифтом', 'reference-reports-09-settings-large.png'),
        ('Качество данных', 'reference-data-05-current-quality-evidence.png'),
        ('Узкое окно, 390 px', 'reference-reports-17-summary-390.png'),
    ]
    figures = []
    for title, filename in screens:
        png = (Path(output) / filename).read_bytes()
        if not png.startswith(b'\x89PNG\r\n\x1a\n'):
            raise ValueError('Invalid screenshot: ' + filename)
        image = base64.b64encode(png).decode('ascii')
        title = html.escape(title)
        figures.append(f'<figure><figcaption>{title}</figcaption><img alt="{title}" loading="lazy" src="data:image/png;base64,{image}"></figure>')
    document = ('<!doctype html><html lang="ru"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">'
                '<title>Анализ работ — установленное приложение</title>'
                '<style>body{font:1rem system-ui;background:#141a21;color:#edf2f7;max-width:85rem;margin:auto;padding:1.5rem}figure{margin:2rem 0}figcaption{font-weight:650;margin:.5rem 0}img{display:block;max-width:100%;height:auto}code{overflow-wrap:anywhere}</style>'
                '<h1>Анализ работ</h1><p>Снимки установленной диагностической сборки на синтетической базе SQLite.</p>'
                '<p>Коммит <code>' + html.escape(revision) + '</code> · ' + html.escape(webview) + '</p>' + ''.join(figures) + '</html>')
    (Path(output) / 'reference-screens.html').write_text(document, encoding='utf-8')
