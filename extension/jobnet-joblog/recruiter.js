// On Recruiter's own pages: says the extension is installed, so the page's
// "Add the extension" banner (modules/joblog_extension) stays hidden. Runs
// at document_start, before the page's scripts, and touches nothing else.
document.documentElement.dataset.jobnetJoblog = (globalThis.browser ?? globalThis.chrome).runtime.getManifest().version;
