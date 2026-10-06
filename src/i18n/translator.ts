/** Exact and templated UI messages take priority over fragment translation. */
export function createTranslator(replacements: readonly (readonly [string, string])[]) {
  const normalize = (value: string) => value.replace(/\s+/g, ' ').trim();
  const exact = new Map(replacements);
  const normalized = new Map(replacements.map(([source, target]) => [normalize(source), target]));
  const placeholder = /\{[^{}]*\}/g;
  const escape = (value: string) => value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const templates = replacements.filter(([source]) => /\{[^{}]*\}/.test(source))
    .sort((a, b) => b[0].length - a[0].length)
    .map(([source, target]) => ({
      pattern: new RegExp(`^${normalize(source).split(placeholder).map(escape).join('(.*?)')}$`, 'u'),
      target,
    }));
  const fragments = replacements.filter(([source]) => /[\u4e00-\u9fff（）：「」【】、，。；]/.test(source))
    .sort((a, b) => b[0].length - a[0].length);

  return (value: string): string => {
    if (!value) return value;
    const leading = value.match(/^\s*/)?.[0] ?? '';
    const trailing = value.match(/\s*$/)?.[0] ?? '';
    const core = value.trim();
    const whole = exact.get(core) ?? normalized.get(normalize(core));
    if (whole !== undefined) return leading + whole + trailing;
    for (const { pattern, target } of templates) {
      const match = pattern.exec(normalize(core));
      if (match) {
        let capture = 1;
        return leading + target.replace(placeholder, () => match[capture++] ?? '') + trailing;
      }
    }
    if (!/[\u4e00-\u9fff]/.test(value)) return value;
    let translated = value;
    for (const [source, target] of fragments) {
      translated = translated.split(source).join(target);
    }
    return translated;
  };
}
