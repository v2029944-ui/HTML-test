"""Generate source fixture before the measured Rust process; no custom limits."""
import pathlib, sys, zipfile
root = pathlib.Path(sys.argv[1]); root.mkdir()
rows, width = int(sys.argv[2]), int(sys.argv[3])
assert 0 < rows <= 59998 and 0 < width < 65500
with zipfile.ZipFile(root / "source.xlsx", "w", compression=zipfile.ZIP_DEFLATED) as z:
    with z.open("xl/worksheets/sheet1.xml", "w") as out:
        out.write(f'<worksheet><sheetData><row r="2"><c r="A2" t="inlineStr"><is><t>СМУ16</t></is></c><c r="B2"><v>{rows}</v></c><c r="C2"><v>{rows * 2}</v></c></row>'.encode())
        for n in range(3, rows + 3):
            # Mixed deterministic ASCII avoids unrealistically extreme compression ratios.
            name = ''.join(chr(33 + ((n * 37 + i * 13 + i * i) % 90)) for i in range(width))
            name = name.replace('&', 'a').replace('<', 'b').replace('>', 'c')
            out.write(f'<row r="{n}"><c r="A{n}" t="inlineStr"><is><t>{name}</t></is></c><c r="B{n}"><v>1</v></c><c r="C{n}"><v>2</v></c></row>'.encode())
        out.write(b'</sheetData></worksheet>')
print(root, 'source_bytes', (root / 'source.xlsx').stat().st_size)
