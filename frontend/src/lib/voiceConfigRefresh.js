export function refreshVoiceConfiguration(initial, fetchConfig, applyConfig, options = {}) {
  const now = options.now || Date.now;
  const schedule = options.schedule || setTimeout;
  const cancel = options.cancel || clearTimeout;
  let timer, stopped = false;
  const plan = (config) => {
    if (stopped || !config.expiresAt) return;
    const expires = Date.parse(config.expiresAt);
    if (!Number.isFinite(expires)) return;
    timer = schedule(refresh, Math.max(1000, expires - now() - 5 * 60 * 1000));
  };
  const refresh = async () => {
    try {
      const config = await fetchConfig();
      if (stopped) return;
      applyConfig(config);
      plan(config);
    } catch {
      if (!stopped) timer = schedule(refresh, 30000);
    }
  };
  plan(initial);
  return () => { stopped = true; cancel(timer); };
}
