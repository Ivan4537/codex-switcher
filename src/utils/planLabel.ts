const FRIENDLY_PLAN_LABELS: Record<string, string> = {
    self_serve_business_prolite: 'Business ProLite',
    'business prolite': 'Business ProLite',
};

/** Convert upstream plan identifiers into concise labels for user-facing UI. */
export function formatPlanLabel(plan?: string | null): string {
    const value = plan?.trim();
    if (!value) return '';
    return FRIENDLY_PLAN_LABELS[value.toLowerCase()] ?? value.toUpperCase();
}
