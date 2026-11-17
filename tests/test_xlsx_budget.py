"""Real-browser proof of XLSX cell accounting without raising import limits."""
import io
import unittest
import zipfile
from pathlib import Path
from playwright.sync_api import sync_playwright

ROOT = Path(__file__).resolve().parents[1]


def workbook_bytes(sheets):
    files = {
        'xl/workbook.xml': '<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>' + ''.join(f'<sheet name="Sheet{i}" r:id="r{i}"/>' for i in range(len(sheets))) + '</sheets></workbook>',
        'xl/_rels/workbook.xml.rels': '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">' + ''.join(f'<Relationship Id="r{i}" Target="worksheets/sheet{i}.xml"/>' for i in range(len(sheets))) + '</Relationships>',
    }
    for i, rows in enumerate(sheets):
        files[f'xl/worksheets/sheet{i}.xml'] = '<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>' + rows + '</sheetData></worksheet>'
    result = io.BytesIO()
    with zipfile.ZipFile(result, 'w', zipfile.ZIP_DEFLATED) as book:
        for name, value in files.items():
            book.writestr(name, value)
    return result.getvalue()


class XlsxBudget(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.pw = sync_playwright().start()
        cls.browser = cls.pw.chromium.launch(executable_path='/usr/bin/chromium', headless=True)

    @classmethod
    def tearDownClass(cls):
        cls.browser.close()
        cls.pw.stop()

    def setUp(self):
        self.page = self.browser.new_page()
        # Load only the actual embedded XLSX reader, not the dashboard's storage/UI.
        html = (ROOT / 'index.html').read_text()
        self.reader = html.split('<script>')[1].split('</script>')[0]
        self.page.add_script_tag(content=self.reader)

    def tearDown(self):
        self.page.close()

    def test_values_formatting_and_uncached_formulas_are_distinguished(self):
        rows = '<row r="1"><c r="A1"><v>0</v></c><c r="B1" t="b"><v>0</v></c><c r="C1" s="1"/><c r="D1" t="inlineStr"><is><t/></is></c><c r="E1"><f>1+1</f><v/></c><c r="F1"><f>2+2</f><v>4</v></c><c r="G1" t="inlineStr"><is><t>Работа</t></is></c><c r="H1"><v>0.5</v></c></row>'
        other = '<row r="1"><c r="A1"/><c r="B1"><f>3+3</f></c></row>'
        actual = self.page.evaluate('''bytes=>{
          const b=XLSX.read(new Uint8Array(bytes));
          return {stats:b.readStats,values:b.Sheets.Sheet0[0]};
        }''', list(workbook_bytes([rows, other])))
        self.assertEqual(actual['values'], [0, False, None, '', None, 4, 'Работа', 0.5])
        self.assertEqual(actual.get('stats'), {
            'cells': 10, 'filledCells': 5, 'emptyCells': 3, 'uncachedFormulaCells': 2,
        })

    def test_budget_is_shared_between_sheets_and_formatting_still_counts(self):
        # Exercise the same production guard with a small boundary. This does not
        # claim a memory measurement or weaken the production 1,500,000 limit.
        self.assertEqual(self.reader.count('++budget.cells>1500000'), 1)
        self.page.add_script_tag(content=self.reader.replace('++budget.cells>1500000', '++budget.cells>3'))
        three = '<row r="1"><c r="A1"/><c r="B1"/><c r="C1"/></row>'
        one = '<row r="1"><c r="A1"/></row>'
        boundary = self.page.evaluate('bytes=>XLSX.read(new Uint8Array(bytes)).readStats', list(workbook_bytes([three])))
        self.assertEqual(boundary, {'cells': 3, 'filledCells': 0, 'emptyCells': 3, 'uncachedFormulaCells': 0})
        error = self.page.evaluate('''bytes=>{try{XLSX.read(new Uint8Array(bytes));return null;}catch(e){return e.message;}}''', list(workbook_bytes([three, one])))
        self.assertIn('пустые оформленные', error)
        self.assertNotIn('1 500 000 заполненных', error)


if __name__ == '__main__':
    unittest.main()
