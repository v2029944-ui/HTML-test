// Controller unit tests capture the bounded SQL callback. Chromium tests use
// the real scrollbar, its layout and native keyboard/mouse input.
module.exports={
 attach(root,spec){root.scrollWindow=spec;return root;},
 reset(root){if(root)root.scrollWindow=null;},
 retire(...controls){for(const control of controls)control.hidden=true;}
};
