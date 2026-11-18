"""Create a small valid XLSX sample for the declarative metric profiles.

Standard-library only. The output must be a NEW file; this never overwrites data.
Usage: python tauri/examples/make_metric_xlsx.py C:/Temp/example-labor.xlsx
"""
from pathlib import Path
from xml.sax.saxutils import escape
import argparse
import zipfile


def create_example(destination: Path, kind: str = 'labor') -> None:
    destination = Path(destination)
    if kind not in ('labor', 'plan_fact'):
        raise ValueError('Choose labor or plan_fact')
    if destination.suffix.lower() != '.xlsx' or not destination.parent.is_dir():
        raise ValueError('Destination must be a new XLSX file in an existing directory')
    headers = ('Подразделение', 'Работа', 'Часы', 'Объём') if kind == 'labor' else (
        'Подразделение', 'Показатель', 'План', 'Факт'
    )
    rows = [
        headers,
        ('СМУ16', 'Монтаж', 8, 16) if kind == 'labor' else ('СМУ16', 'Квартал', 100, 70),
        ('АВТО', 'Ремонт', 4, 3) if kind == 'labor' else ('АВТО', 'Квартал', 200, 190),
    ]
    pieces = []
    for row_number, row in enumerate(rows, 1):
        cells = []
        for index, value in enumerate(row):
            addr = f'{chr(ord("A") + index)}{row_number}'
            if isinstance(value, str):
                cells.append(f'<c r="{addr}" t="inlineStr"><is><t>{escape(value)}</t></is></c>')
            else:
                cells.append(f'<c r="{addr}"><v>{value}</v></c>')
        pieces.append(f'<row r="{row_number}">{"".join(cells)}</row>')
    sheet = ('<?xml version="1.0" encoding="UTF-8"?>'
             '<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">'
             f'<sheetData>{"".join(pieces)}</sheetData></worksheet>')
    content_types = (
        '<?xml version="1.0" encoding="UTF-8"?>'
        '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
        '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
        '<Default Extension="xml" ContentType="application/xml"/>'
        '<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>'
        '<Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>'
        '</Types>'
    )
    root_rels = (
        '<?xml version="1.0" encoding="UTF-8"?>'
        '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
        '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>'
        '</Relationships>'
    )
    workbook = (
        '<?xml version="1.0" encoding="UTF-8"?>'
        '<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" '
        'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">'
        '<sheets><sheet name="Data" sheetId="1" r:id="rId1"/></sheets></workbook>'
    )
    workbook_rels = (
        '<?xml version="1.0" encoding="UTF-8"?>'
        '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
        '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>'
        '</Relationships>'
    )
    # Mode 'x' guarantees no existing workbook is overwritten.
    with zipfile.ZipFile(destination, 'x', zipfile.ZIP_DEFLATED) as output:
        for name, content in [
            ('[Content_Types].xml', content_types),
            ('_rels/.rels', root_rels),
            ('xl/workbook.xml', workbook),
            ('xl/_rels/workbook.xml.rels', workbook_rels),
            ('xl/worksheets/sheet1.xml', sheet),
        ]:
            output.writestr(name, content)


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('destination', type=Path)
    parser.add_argument('--kind', choices=('labor', 'plan_fact'), default='labor')
    args = parser.parse_args()
    create_example(args.destination, args.kind)
    print(f'Created sample XLSX: {args.destination}')
