'use strict';
// Build-time sharing keeps the standalone classic UI the appearance source.
function between(source,start,end){const a=source.indexOf(start),b=source.indexOf(end,a+start.length);if(a<0||b<0)throw Error('Classic UI source markers changed: '+start);return source.slice(a,b);}
function withoutClassicPrint(css){
 let result='',offset=0;
 const media=/@media\s+print\s*\{/g;let match;
 while((match=media.exec(css))){result+=css.slice(offset,match.index);let depth=1,i=media.lastIndex;while(i<css.length&&depth){if(css[i]==='{')depth++;if(css[i]==='}')depth--;i++;}if(depth)throw Error('Unclosed classic print CSS');offset=i;media.lastIndex=i;}
 return result+css.slice(offset);
}
function classicAssets(source){
 const css=withoutClassicPrint(between(source,'<style>','</style>').slice('<style>'.length));
 const appearance=between(source,'const THEME_KEY=','function initChartTooltip()');
 const markup=between(source,'      <section class="panel" aria-labelledby="appearanceHeading">','      </section>')+'      </section>';
 const script=`// Generated from the classic appearance preferences; no report renderer.\n(function(){'use strict';const $=id=>document.getElementById(id);if(!$('appearanceTheme'))return;function notice(text){const out=$('appearanceSaveStatus');if(out)out.textContent=text;}function redrawCharts(){if(typeof window.dispatchEvent==='function'&&typeof CustomEvent==='function')window.dispatchEvent(new CustomEvent('analiz-appearance-change'));}\n${appearance}\ninitTheme();})();\n`;
 return {css,script,markup};
}
module.exports={classicAssets};
