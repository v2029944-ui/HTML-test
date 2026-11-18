"""Create a bounded source-issue/single-report fixture before the measured Rust child."""
import itertools, pathlib, sys, zipfile
root=pathlib.Path(sys.argv[1]);root.mkdir()
rows=int(sys.argv[2]);assert 1<=rows<=30000
with zipfile.ZipFile(root/'source.xlsx','w',zipfile.ZIP_DEFLATED) as z:
    with z.open('xl/worksheets/sheet1.xml','w') as out:
        out.write('<worksheet><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Работа</t></is></c><c r="B1" t="inlineStr"><is><t>2024-02-28</t></is></c><c r="C1" t="inlineStr"><is><t>2024-02-28</t></is></c><c r="D1" t="inlineStr"><is><t>2024-02-29</t></is></c><c r="E1" t="inlineStr"><is><t>2024-02-29</t></is></c><c r="F1" t="inlineStr"><is><t>Итого за период</t></is></c><c r="G1" t="inlineStr"><is><t>Итого за период</t></is></c></row><row r="2"><c r="B2" t="inlineStr"><is><t>Количество</t></is></c><c r="C2" t="inlineStr"><is><t>Сумма</t></is></c><c r="D2" t="inlineStr"><is><t>Количество</t></is></c><c r="E2" t="inlineStr"><is><t>Сумма</t></is></c><c r="F2" t="inlineStr"><is><t>Количество</t></is></c><c r="G2" t="inlineStr"><is><t>Сумма</t></is></c></row>'.encode())
        for r,label in itertools.chain([(3,'СМУ16'),(4,'Valid')],((i+5,'Bad '+str(i)) for i in range(rows))):
            value='<is><t>bad</t></is>' if r>=5 else '<v>1</v>'
            kind=' t="inlineStr"' if r>=5 else ''
            out.write(f'<row r="{r}"><c r="A{r}" t="inlineStr"><is><t>{label}</t></is></c><c r="B{r}"{kind}>{value}</c><c r="C{r}"><v>2</v></c><c r="F{r}"><v>1</v></c><c r="G{r}"><v>2</v></c></row>'.encode())
        out.write(b'</sheetData></worksheet>')
    z.writestr('xl/workbook.xml','<workbook xmlns:r="r"><sheets><sheet name="Data" r:id="r1"/></sheets></workbook>')
    z.writestr('xl/_rels/workbook.xml.rels','<Relationships><Relationship Id="r1" Target="worksheets/sheet1.xml" Type="worksheet"/></Relationships>')
print('source_bytes', (root/'source.xlsx').stat().st_size,'invalid_rows',rows)
