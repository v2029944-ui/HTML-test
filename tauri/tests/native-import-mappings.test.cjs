'use strict';const test=require('node:test'),assert=require('node:assert/strict');
const {parseMappings}=require('../native-import-mappings.js');
test('maps explicit Excel columns and one-based header rows to bounded native triples',()=>{
 assert.equal(parseMappings(''),null);
 assert.deepEqual(parseMappings('1; A; B; C\n4; AA; AC; AD'),[
 {header_row:1,work_col:1,count_col:2,amount_col:3},
 {header_row:4,work_col:27,count_col:29,amount_col:30}]);
 for(const text of ['0;A;B;C','1;A;A;C','1;A;B','1;A;B;C\n1;A;B;C','100001;A;B;C','1;ZZZ;B;C'])assert.throws(()=>parseMappings(text),text);
});
