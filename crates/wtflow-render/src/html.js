document.getElementById('expand').onclick = () => document.querySelectorAll('details.flow').forEach(e => e.open = true);
document.getElementById('collapse').onclick = () => document.querySelectorAll('details.flow,.metadata').forEach(e => e.open = e.classList.contains('overview'));
const key = 'wtflow:notes:' + document.getElementById('note-store').dataset.flow;
const status = document.getElementById('note-status');
let saved = Object.create(null);
try {
  const data = JSON.parse(localStorage.getItem(key) || '{}');
  if (data && typeof data === 'object' && !Array.isArray(data)) Object.assign(saved, data);
} catch (_) { status.textContent = 'Browser storage is unavailable. Export notes to keep them.'; }
const notes = [...document.querySelectorAll('.note-input')];
for (const input of notes) {
  const previous = saved[input.dataset.id];
  if (previous && previous.code === input.dataset.code && typeof previous.value === 'string') input.value = previous.value;
  if (input.value) input.closest('details').open = true;
  input.addEventListener('input', () => {
    saved[input.dataset.id] = {code: input.dataset.code, value: input.value};
    try {
      localStorage.setItem(key, JSON.stringify(saved));
      status.textContent = 'Notes saved in this browser. Export for a portable copy.';
    } catch (_) { status.textContent = 'Could not save in this browser. Export notes to keep them.'; }
  });
}
document.getElementById('export-notes').onclick = () => {
  const labels = Object.create(null);
  for (const input of notes) if (input.value || Object.prototype.hasOwnProperty.call(saved, input.dataset.id)) labels[input.dataset.id] = input.value;
  const blob = new Blob([JSON.stringify(labels, null, 2) + '\n'], {type: 'application/yaml'});
  const url = URL.createObjectURL(blob);
  const link = document.createElement('a');
  link.href = url;
  link.download = 'labels.yaml';
  link.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
  status.textContent = 'Exported labels.yaml. Apply with wtflow label FLOW labels.yaml to include notes in the flow document.';
};
