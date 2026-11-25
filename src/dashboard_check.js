const checkErrors = [];
window.addEventListener('error', event => checkErrors.push(event.message || event.target?.src || 'asset error'), true);
window.addEventListener('securitypolicyviolation', event => checkErrors.push(event.violatedDirective + ': ' + event.blockedURI));
window.addEventListener('DOMContentLoaded', async () => {
  if (!window.pywebview) return;
  const checks = [];
  const api = window.pywebview.api;
  const assert = (condition, name) => {
    if (!condition) throw new Error(name);
    checks.push(name);
  };
  const element = (selector) => document.querySelector(selector);
  const waitFor = async (predicate, name) => {
    const end = Date.now() + 3500;
    while (!predicate() && Date.now() < end) {
      await new Promise(resolve => setTimeout(resolve, 60));
    }
    assert(predicate(), name);
  };
  const openTab = name => element('nav button[data-page="' + name + '"]').click();
  try {
    await waitFor(() => element('.page.active')?.dataset.page === 'dictate', 'Dictate opened from the actual Rust bridge');
    await waitFor(() => element('#dictate-heading').textContent.includes('Ready'), 'Ready state rendered');
    await waitFor(() => element('.brand-mark')?.naturalWidth === 128, 'Rust logo loads in the dashboard');

    openTab('settings');
    await waitFor(() => element('#microphone-note')?.hidden === false, 'Disconnected saved microphone visible');
    element('#use-default-microphone').click();
    await waitFor(() => element('#microphone-note')?.hidden, 'Windows default cleared the disconnected state');
    const preview = element('[data-toggle="live_preview"]');
    const before = preview.getAttribute('aria-checked');
    preview.focus();
    assert(document.activeElement === preview, 'Preview switch takes focus');
    preview.click();
    await waitFor(() => preview.getAttribute('aria-checked') !== before, 'Preview switch updates through Rust');
    preview.click();
    await waitFor(() => preview.getAttribute('aria-checked') === before, 'Preview switch restores its saved value');
    const cmdHotkey = element('select[data-key="command_hotkey"]');
    assert(cmdHotkey && cmdHotkey.options[0]?.value === '', 'Command Mode hotkey selector renders with Off default');
    const quiet = element('[data-toggle="quiet_mode"]');
    const quietBefore = quiet.getAttribute('aria-checked');
    quiet.click();
    await waitFor(() => quiet.getAttribute('aria-checked') !== quietBefore, 'Quiet / whisper mode switch updates through Rust');
    quiet.click();
    await waitFor(() => quiet.getAttribute('aria-checked') === quietBefore, 'Quiet / whisper mode switch restores its saved value');

    element('[data-settings-tab="appearance"]').click();
    await waitFor(() => element('[data-settings-pane="appearance"]')?.classList.contains('is-active'), 'Appearance category opens');
    const colorChips = document.querySelectorAll('#appearance-swatches .theme-swatch');
    const layoutCards = document.querySelectorAll('#appearance-layouts .layout-card');
    assert(colorChips.length === 12 && layoutCards.length === 3, 'Appearance offers all color schemes and layouts');
    const pickTheme = key => element(`#appearance-swatches [data-theme-choice="${key}"]`);
    const pickLayout = key => element(`#appearance-layouts [data-layout-choice="${key}"]`);
    const originalBackground = getComputedStyle(document.body).backgroundColor;
    const paperChip = pickTheme('paper');
    paperChip.focus();
    paperChip.click();
    assert(document.activeElement === paperChip, 'Color picker keeps keyboard focus while saving');
    await waitFor(() => document.documentElement.dataset.theme === 'paper', 'Color scheme switches through Rust');
    assert(document.documentElement.dataset.mode === 'light', 'Light theme sets data-mode=light');
    assert(getComputedStyle(document.body).backgroundColor !== originalBackground, 'Color scheme repaints the dashboard');
    assert(element('.brand-mark').naturalWidth === 128, 'Rust logo remains visible after changing theme');
    pickLayout('sidebar').click();
    await waitFor(() => document.documentElement.dataset.layout === 'sidebar', 'Layout switches through Rust');
    assert(getComputedStyle(element('.rail')).flexDirection === 'column', 'Sidebar places navigation vertically');
    assert(getComputedStyle(element('.settings-rail')).flexDirection === 'row', 'Settings switches to horizontal rail in Sidebar layout');
    pickLayout('compact').click();
    await waitFor(() => document.documentElement.dataset.layout === 'compact', 'Compact layout can be selected');
    pickTheme('espresso').click();
    await waitFor(() => document.documentElement.dataset.theme === 'espresso' && document.documentElement.dataset.mode === 'dark', 'Espresso dark theme switches through Rust');
    pickTheme('mist').click();
    await waitFor(() => document.documentElement.dataset.theme === 'mist' && document.documentElement.dataset.mode === 'light', 'Mist light theme switches through Rust');
    pickTheme('warm').click();
    await waitFor(() => document.documentElement.dataset.theme === 'warm', 'Warm charcoal can be restored');
    element('#appearance-theme-next').click();
    await waitFor(() => document.documentElement.dataset.theme === 'paper', 'Next arrow advances to the next color scheme');
    element('#appearance-theme-prev').click();
    await waitFor(() => document.documentElement.dataset.theme === 'warm', 'Previous arrow returns to Warm charcoal');
    pickLayout('desk').click();
    await waitFor(() => document.documentElement.dataset.layout === 'desk', 'Writer’s desk can be restored');

    element('[data-settings-tab="models"]').click();
    await waitFor(() => element('[data-settings-pane="models"]')?.classList.contains('is-active'), 'Voice & AI category opens');
    element('#btn-toggle-speech-models').click();
    element('#btn-toggle-cleanup-models').click();
    assert(!element('#speech-catalog-wrap').hidden && !element('#cleanup-catalog-wrap').hidden, 'Model drawers expand when toggled');
    await waitFor(() => document.querySelectorAll('#speech-catalog-list .catalog-item').length >= 2, 'Settings renders local speech model catalog');
    await waitFor(() => document.querySelectorAll('#cleanup-catalog-list .catalog-item').length >= 2, 'Settings renders local formatting model catalog');
    const sampleInstall = await api.install_model('cleanup', 'qwen3.5:4b');
    assert(sampleInstall && sampleInstall.ok === false, 'Sample mode blocks model downloads');
    const sampleBench = await api.start_benchmark('');
    assert(sampleBench && sampleBench.ok === false, 'Sample mode blocks model benchmarks');
    const appTone = element('[data-toggle="auto_app_tone"]');
    const toneBefore = appTone.getAttribute('aria-checked');
    appTone.click();
    await waitFor(() => appTone.getAttribute('aria-checked') !== toneBefore, 'App-aware tone switch updates through Rust');
    appTone.click();
    await waitFor(() => appTone.getAttribute('aria-checked') === toneBefore, 'App-aware tone switch restores its saved value');
    element('#btn-toggle-tone-rules').click();
    assert(!element('#tone-rules-wrap').hidden && element('#tone-app-input') && element('#tone-preset-select'), 'App-aware tone rules drawer expands cleanly');

    element('[data-settings-tab="snippets"]').click();
    await waitFor(() => element('[data-settings-pane="snippets"]')?.classList.contains('is-active'), 'Snippets category opens');
    assert(element('#snippet-trigger') && element('#snippet-expansion') && element('#btn-add-snippet') && element('#snippets-list'), 'Snippets tab renders add form and list');

    const settingsFilter = element('#settings-filter');
    settingsFilter.value = 'qwen';
    settingsFilter.dispatchEvent(new Event('input', { bubbles: true }));
    await waitFor(() => element('[data-settings-pane="models"]')?.classList.contains('is-active') && !element('#settings-stage').hidden, 'Settings filter matches model names');
    settingsFilter.value = 'shortcut';
    settingsFilter.dispatchEvent(new Event('input', { bubbles: true }));
    await waitFor(() => element('[data-settings-pane="system"]')?.classList.contains('is-active'), 'Settings filter jumps to matching category');
    settingsFilter.value = 'zzz-no-match';
    settingsFilter.dispatchEvent(new Event('input', { bubbles: true }));
    await waitFor(() => element('#settings-stage').hidden && !element('#settings-filter-empty').hidden, 'Empty filter hides stale settings pane');
    settingsFilter.value = '';
    settingsFilter.dispatchEvent(new Event('input', { bubbles: true }));
    await waitFor(() => !element('#settings-stage').hidden, 'Clearing filter restores settings pane');
    element('[data-settings-tab="general"]').click();

    openTab('lab');
    const draft = element('#draft');
    const sample = 'Isolated Rust dashboard draft.';
    const pass = element('#btn-draft-pass');
    assert(pass.getAttribute('aria-disabled') === 'true', 'Engineer is unavailable for an empty draft');
    draft.value = sample;
    draft.dispatchEvent(new Event('input', { bubbles: true }));
    const deadline = Date.now() + 3500;
    while ((await api.get_lab()).text !== sample && Date.now() < deadline) {
      await new Promise(resolve => setTimeout(resolve, 70));
    }
    assert((await api.get_lab()).text === sample, 'Prompt Lab editor saved into Rust profile');
    pass.click();
    await waitFor(() => draft.value === 'Engineered dashboard draft.' && pass.textContent === 'Refine', 'Engineer publishes a scored revision and offers Refine');
    assert(element('#score-badge').textContent === 'Score 8/10', 'Engineer shows the checking model’s score');
    element('#btn-copy').click();
    await waitFor(() => document.body.textContent.includes('Copied draft.'), 'Copy accepts the engineered draft in the fixture');
    pass.click();
    await waitFor(() => draft.value === 'Refined dashboard draft.', 'Refine publishes a higher-scoring revision');
    assert(element('#score-badge').textContent === 'Score 9/10', 'Refine shows the higher score');
    pass.click();
    await waitFor(() => element('#lab-busy').textContent.includes('No meaningful improvement'), 'Refine keeps the existing draft when there is no improvement');

    element('.legacy-lab-tools').open = true;
    const composer = element('#composer');
    composer.value = 'Add a verification step';
    composer.dispatchEvent(new Event('input', { bubbles: true }));
    element('#btn-send').click();
    await waitFor(() => element('#review-action-label').textContent === 'Compare suggestion', 'Write suggestion saves a separate proposal');
    await waitFor(() => !element('#btn-review').classList.contains('hidden'), 'Compare opens for the saved suggestion');
    element('#btn-review').click();
    await waitFor(() => element('#candidate').open, 'Suggestion review dialog is visible');
    assert(element('#candidate-original').value === 'Refined dashboard draft.'
      && element('#candidate-text').value === 'Synthetic suggestion for review.', 'Compare shows the unchanged draft and the suggestion');
    element('#btn-review-suggestion').click();
    await waitFor(() => element('#candidate-text').value === 'Reviewed suggestion.', 'Review creates a second version without changing the draft');
    assert(draft.value === 'Refined dashboard draft.', 'The draft remains intact until Apply');
    element('#btn-apply').click();
    await waitFor(() => !element('#candidate').open && draft.value === 'Reviewed suggestion.', 'Apply saves the selected suggestion as a new revision');
    element('#btn-score').click();
    await waitFor(() => element('#review-action-label').textContent === 'View feedback'
      && element('#score-badge').textContent === 'Score 8/10', 'Get feedback scores the selected revision');
    element('#btn-new').click();
    await waitFor(() => draft.value === '', 'New draft opens after saving the previous draft');
    const archived = (await api.list_drafts()).find(row => row.title.includes('Isolated Rust'));
    assert(archived, 'Previous draft can be found in the archive');
    await waitFor(() => Array.from(element('#archived-drafts').options).some(option => option.value === archived.id), 'Archived draft appears in the picker');
    element('#btn-more').click();
    element('.archived').open = true;
    element('#archived-drafts').value = archived.id;
    element('#btn-restore').click();
    await waitFor(() => draft.value === 'Reviewed suggestion.', 'Restore reopens the previously applied revision');

    openTab('history');
    await waitFor(() => element('[data-delivery="uncertain"]'), 'History keeps saved text and uncertain paste status separate');
    await waitFor(() => element('#history-metric-wpm .metric-digits')?.textContent === '–',
      'History hides speaking speed for a short sample');
    const weeklyWords = element('#history-metric-words_week');
    weeklyWords.focus();
    assert(document.activeElement === weeklyWords && element('#metric-tip-words_week').textContent.includes('vs previous 7 days')
      && element('#metric-tip-words_week .metric-bars')?.children.length === 4,
      'History renders the four-week comparison on a focusable metric');
    assert(element('#metric-tip-wpm').textContent.includes('Not enough history for a trend'),
      'History explains when speaking speed samples are missing');
    element('[data-history-mode="app"]').click();
    await waitFor(() => element('.history-timeline').textContent.includes('Sample editor'), 'History groups by the stored destination');
    const filter = element('#hist-search');
    filter.value = 'nonexistent phrase zyx';
    filter.dispatchEvent(new Event('input', { bubbles: true }));
    await waitFor(() => !element('[data-delivery="uncertain"]'), 'History filters by text');
    assert(element('[data-history-mode="app"]').getAttribute('aria-pressed') === 'true', 'History switches to App groups');

    openTab('meetings');
    await waitFor(() => element('.page.active')?.dataset.page === 'meetings', 'Meetings tab opens');
    await waitFor(() => element('#btn-meeting-start') && element('#meeting-timer') && element('#meeting-status') && element('#meetings-list'), 'Meetings page renders recorder and notes list');
    const meetingsState = await api.get_meetings();
    assert(meetingsState && Array.isArray(meetingsState.notes), 'Meetings bridge returns state and notes');

    openTab('dictate');
    await api.__check_recording();
    await waitFor(() => !element('#btn-cancel').disabled, 'Recording enables Finish and Cancel');
    await waitFor(() => element('#live-preview-text').textContent.length > 0, 'Recording words reach the shared Dictate screen');
    element('#btn-quit').click();
    await waitFor(() => element('#toasts').textContent.includes('Finish or cancel the recording'),
      'Quit explains why the recording must finish or be canceled first');
    const lanternDeadline = Date.now() + 3000;
    let lanternVisible = false;
    while (!(lanternVisible = await api.__check_lantern()) && Date.now() < lanternDeadline) {
      await new Promise(resolve => setTimeout(resolve, 90));
    }
    assert(lanternVisible, 'Native Lantern and click-through shadow are visible without taking foreground');
    element('#btn-cancel').click();
    await waitFor(() => element('#btn-cancel').disabled, 'Cancel returns the fixture to idle');
    assert((await api.get_history(true)).length === 2, 'Cancel did not add a History row');
    openTab('lab');
    const draftAtClose = 'Keep the words typed just before closing.';
    draft.value = draftAtClose;
    draft.dispatchEvent(new Event('input', { bubbles: true }));
    await api.__check_done({ passed: true, checks, draftAtClose, view: { width: innerWidth, height: innerHeight } });
  } catch (error) {
    await api.__check_done({
      passed: false, checks, error: String(error), checkErrors,
      detail: { url: location.href, heading: element('#dictate-heading')?.textContent,
        css: !!document.styleSheets.length, active: element('.page.active')?.dataset.page,
        settings: !!(await api.get_settings())?.hotkey },
    });
  }
});
