/* Shared presentation and in-flight reads for the shell and the detailed overview. */
(function () {
  'use strict';
  const titles = Object.freeze({amount:'Сумма', count:'Количество / объём', average:'Сумма / количество (объём)'});
  let current = null;
  function read(api, scope) {
    const {revision,type,source,displayType,from,to,baseFrom,baseTo,organizations} = scope;
    const key = JSON.stringify([window.sharedWorkspace?.state?.workspace_token, revision, source, type, displayType,
      from, to, baseFrom, baseTo, organizations, scope.allNames, scope.selectedNames, scope.selectedKeys]);
    if (current?.key === key) return current.result;
    // These two dashboards both show totals over all works. The work selection
    // is still part of the identity so different filter actions cannot share a pending result.
    const result = Promise.all([api.nativeAnalysisCompare(revision,type,baseFrom,baseTo,from,to,organizations,null,null),
      api.nativeAnalysisOrganizations(revision,type,from,to,organizations,null,null)])
      .finally(() => { if (current?.result === result) current = null; });
    current = {key,result};
    return result;
  }
  window.analysisSummary = Object.freeze({titles,read});
})();
