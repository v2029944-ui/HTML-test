"""Check the geometry of rendered controls, including in the installed WebView2."""
from playwright.sync_api import TimeoutError as PlaywrightTimeoutError


def check_control_layout(page):
    inspect = """() => {
      const errors = [], rect = node => node.getBoundingClientRect();
      const visible = node => node && rect(node).width > 0 && rect(node).height > 0 &&
        getComputedStyle(node).visibility === 'visible';
      const tolerance = 2;
      const titleBox = label => {
        for (const child of label.childNodes) {
          if (!(child.textContent || '').trim() ||
              (child.nodeType !== Node.TEXT_NODE && child.tagName !== 'SPAN')) continue;
          const range = document.createRange();
          range.selectNodeContents(child);
          return range.getClientRects()[0];
        }
      };
      const paint = document.createElement('canvas').getContext('2d');
      const color = value => { paint.fillStyle = value; return paint.fillStyle; };
      const close = (a,b) => Math.abs(a-b) <= tolerance;
      if (innerWidth >= 720) {
        const tables = document.querySelectorAll('#s1 .table-viewport table,#s2 .changes-table,#refNative[data-panel] .metric-table,#refNative[data-panel] .table-scroll table');
        for (const table of tables) {
          if (!visible(table)) continue;
          for (const cell of table.querySelectorAll('thead tr:first-child > :is(th,td),tbody tr:first-child > :is(td,th)')) {
            if (visible(cell) && getComputedStyle(cell).textAlign !== 'center')
              errors.push([table.caption?.innerText || table.className,cell.textContent.slice(0,60),'cell content not centered']);
          }
        }
      }
      const parameterGrid = document.querySelector('#uploadParameterCard > .import-grid');
      if (visible(parameterGrid)) {
        const source = document.querySelector('#uploadEntryCard');
        const parameters = parameterGrid.parentElement;
        const sourceButton = source.querySelector('#uploadSourceFile');
        const periodType = parameterGrid.querySelector('#importType');
        if (visible(sourceButton) && visible(periodType) && close(rect(source).top,rect(parameters).top) &&
            (!close(rect(sourceButton).top,rect(periodType).top) || !close(rect(sourceButton).bottom,rect(periodType).bottom)))
          errors.push(['Источник Excel', 'source buttons and first report field start at different heights']);
        const rowFields = [];
        for (const group of parameterGrid.children) {
          const field = [...group.querySelectorAll('select,.ref-calendar-field')].find(visible);
          if (!field) continue;
          for (const {groupTop,fieldTop} of rowFields) {
            if (close(groupTop,rect(group).top) && !close(fieldTop,rect(field).top))
              errors.push([field.id,'import fields with adjacent captions start at different heights']);
          }
          rowFields.push({groupTop:rect(group).top,fieldTop:rect(field).top});
          if (field.tagName === 'SELECT' && CSS.supports('appearance','base-select')) {
            const style = getComputedStyle(field);
            if (!style.display.includes('flex') || style.alignItems !== 'center')
              errors.push([field.id,'closed select does not center its text']);
          }
        }
      }
      const quality = document.querySelector('#qualityTable table');
      if (visible(quality)) {
        for (const cell of quality.querySelectorAll('th, tbody tr:first-child td:first-child')) {
          const text = document.createRange(); text.selectNodeContents(cell.firstChild);
          if (text.getClientRects().length > 1) errors.push([cell.innerText,'wrapped quality header or report period']);
          const help = cell.querySelector('.table-help-button');
          if (help && rect(help).top >= text.getBoundingClientRect().bottom)
            errors.push([cell.innerText,'help button below its column heading']);
        }
      }
      const reportButtons = document.querySelector('#reports .calendar-source');
      if (visible(reportButtons)) {
        const actions = [...reportButtons.querySelectorAll('button')];
        const quality = actions.find(button => button.textContent === 'Контроль качества');
        const remove = actions.find(button => button.textContent === 'Удалить отчёт');
        if (visible(quality) && visible(remove) &&
            Math.max(rect(remove).left-rect(quality).right, rect(remove).top-rect(quality).bottom) < 3)
          errors.push(['report actions','no gap between the quality and delete buttons']);
      }
      const fields = [...document.querySelectorAll('#refNative label > .ref-calendar-field')];
      for (const field of fields) {
        if (!visible(field)) continue;
        const parent = field.parentElement, p = rect(parent), f = rect(field), title = titleBox(parent);
        const appearance = getComputedStyle(field);
        const tokens = field.disabled ? ['--subtle'] : ['--field-bg','--hover'];
        if (!tokens.some(token => color(appearance.getPropertyValue(token)) === color(appearance.backgroundColor)) ||
            color(appearance.color) !== color(appearance.getPropertyValue(field.disabled ? '--muted' : '--text')))
          errors.push([field.getAttribute('aria-label'), 'calendar field does not match its enabled state',
            appearance.backgroundColor, appearance.color, tokens.map(t=>appearance.getPropertyValue(t)),
            appearance.getPropertyValue(field.disabled ? '--muted' : '--text'),field.matches(':hover')]);
        if (visible(field.previousElementSibling))
          errors.push([field.getAttribute('aria-label'), 'both calendar controls are visible']);
        const style = getComputedStyle(parent);
        const inset = side => parseFloat(style['padding'+side]) + parseFloat(style['border'+side+'Width']);
        if ((title && f.top < title.bottom - tolerance) ||
            !close(f.left, p.left + inset('Left')) || !close(f.right, p.right - inset('Right')))
          errors.push([field.getAttribute('aria-label'), 'calendar field width or caption position']);
      }
      for (const button of document.querySelectorAll('#refComparisonDates [data-compare-date], #reportShell .date-label-pair > [data-date]')) {
        if (!visible(button)) continue;
        const field = rect(button), parent = rect(button.parentElement);
        const label = button.previousElementSibling;
        if (!label || rect(label).bottom > field.top+tolerance ||
            !close(field.left,parent.left) || !close(field.right,parent.right))
          errors.push([button.textContent, 'calendar button misaligned with period label']);
      }
      const duration = document.querySelector('#reportShell [data-base-duration]');
      const previousPeriod = document.querySelector('#reportShell [data-baseline-quick=prev]');
      if (visible(duration) && visible(previousPeriod) && rect(duration).bottom > rect(previousPeriod).top + tolerance)
        errors.push(['Предыдущий', 'baseline duration overlaps the quick action']);
      const pageScale = {'80':.8,'90':.9}[document.documentElement.dataset.uiSize] ?? 1;
      for (const button of document.querySelectorAll('.sql-pager-controls button')) {
        if (visible(button) && !close(rect(button).height, 28 * pageScale))
          errors.push([button.getAttribute('aria-label'), 'page button differs from compact size',rect(button).height]);
      }
      for (const range of document.querySelectorAll('.report-range')) {
        const [period, baseline, edit] = range.children;
        if (visible(edit) && (rect(edit).bottom < rect(period).top-tolerance ||
            rect(edit).bottom < rect(baseline).top-tolerance))
          errors.push(['Период отчёта', 'edit link displayed above selected dates']);
      }
      // Captions may wrap, including at larger text sizes. Compare the actual
      // glyph positions on each side of a separator instead of parsing innerText.
      for (const label of document.querySelectorAll('.report-range > span,.filter-summary,.source-state,' +
          '.detail-date-scope > span,#changesActiveFilter')) {
        if (!visible(label)) continue;
        for (const text of label.childNodes) {
          if (text.nodeType !== Node.TEXT_NODE) continue;
          const value = text.nodeValue;
          const glyphTop = index => {
            const glyph = document.createRange();
            glyph.setStart(text, index);
            glyph.setEnd(text, index + 1);
            return glyph.getBoundingClientRect().top;
          };
          for (let index = value.indexOf('·'); index >= 0; index = value.indexOf('·', index + 1)) {
            const before = value.slice(0,index).trimEnd().length - 1;
            const after = value.length - value.slice(index+1).trimStart().length;
            if (before < 0 || after === value.length || !close(glyphTop(before), glyphTop(index)) ||
                !close(glyphTop(index), glyphTop(after)))
              errors.push([value, 'separator stranded at a line boundary']);
          }
        }
      }
      const sheet = document.querySelector('#sheet[open]');
      const apply = sheet?.querySelector('form .filter-apply');
      if (visible(apply) && innerWidth >= 720) {
        const cancel = sheet.querySelector('form button:not(.filter-apply)');
        if (!visible(cancel) || !close(rect(cancel).top, rect(apply).top) ||
            rect(apply).bottom > Math.min(rect(sheet).bottom, innerHeight) + tolerance)
          errors.push(['Применить', 'dialog actions must stay together inside the viewport']);
      }
      const metric = document.querySelector('#reportShell [data-measure]');
      if (visible(metric)) {
        for (const next of document.querySelectorAll('#reportShell .filter-options [data-pick]')) {
          if (visible(next) && close(rect(next.parentElement).top, rect(metric.parentElement).top) &&
              (!close(rect(metric).top,rect(next).top) || !close(rect(metric).bottom,rect(next).bottom)))
            errors.push(['Показатель', 'misaligned with adjacent filter']);
        }
        if (innerWidth >= 1050 && !document.querySelector('#refRoot').dataset.filterSidebar) {
          const filterFields = [...document.querySelectorAll('#reportShell :is(.filter-periods,.filter-options) > .lbl > :is(select,button)')].filter(visible);
          if (!filterFields.every(field => close(rect(field).top,rect(metric).top) &&
              close(rect(field).bottom,rect(metric).bottom)))
            errors.push(['Периоды и фильтры', 'period, source, baseline and options are not aligned']);
          const source = document.querySelector('#referenceSource'), baseline = document.querySelector('#sum-base');
          if (rect(source).left >= rect(baseline).left)
            errors.push(['Источник', 'source should precede the comparison baseline']);
        }
      }
      const orientation = document.querySelector('#chartOrientation');
      if (visible(orientation)) {
        const field = rect(orientation), modes = rect(document.querySelector('.chart-mode-controls'));
        if (field.left >= modes.right - tolerance &&
            (!close(field.top,modes.top) || !close(field.bottom,modes.bottom)))
          errors.push(['Расположение столбцов', 'misaligned with chart modes']);
      }
      for (const id of ['replaceExisting','unifiedAcceptOverlap','unifiedAcceptQuality']) {
        const box = document.getElementById(id);
        if (!visible(box)) continue;
        const r = rect(box), text = titleBox(box.parentElement);
        if (!text || text.left-r.right < 0 || text.left-r.right > 16 || r.width > 28 ||
            Math.abs((r.top+r.bottom-text.top-text.bottom)/2)>3)
          errors.push([id,'checkbox separated from its caption']);
      }
      return errors;
    }"""
    # Moving the cursor or scrolling a control into view starts a 240 ms color
    # transition. Poll its rendered state instead of sampling an intermediate RGB.
    try:
        page.wait_for_function("() => (" + inspect + ")().length === 0", timeout=3000)
    except PlaywrightTimeoutError:
        failures = page.evaluate(inspect)
        assert not failures, failures
