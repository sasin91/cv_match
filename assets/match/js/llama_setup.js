// The setup wizard's progress bar: while a download runs, ask llama_setup/progress how far it is
// and move the bar; when it has stopped (done or failed), reload the page once to show what's next.
(function () {
    const box = document.querySelector('.download-progress');
    if (!box) {
        return;
    }
    const bar = box.querySelector('progress');
    const text = box.querySelector('[data-progress-text]');
    const url = box.dataset.progressUrl;
    const every = Number(box.dataset.pollMilliseconds);

    async function poll() {
        try {
            const response = await fetch(url, { cache: 'no-store' });
            const answer = await response.json();
            if (!answer.running) {
                location.reload();
                return;
            }
            if (answer.percent === null) {
                bar.removeAttribute('value');
            } else {
                bar.value = answer.percent;
            }
            text.textContent = answer.progress;
        } catch (e) {
            console.error('llama_setup: the progress could not be read', e);
            text.textContent = 'The progress could not be read: ' + e;
        }
        setTimeout(poll, every);
    }

    setTimeout(poll, every);
})();
