// Runs only on the official first-party page. Never solves verification or sends invites.
if (location.hostname !== 'chatgpt.com' || location.protocol !== 'https:') return;
function mount() {
    if (!document.body || document.getElementById('switcher-referral-read')) return;
    const panel = document.createElement('div');
    panel.id = 'switcher-referral-read';
    Object.assign(panel.style, { position: 'fixed', top: '12px', left: '12px', right: '12px', zIndex: '2147483647', background: '#fff', color: '#111', border: '2px solid #2563eb', padding: '16px', font: '14px sans-serif' });
    const note = document.createElement('p');
    note.textContent = '只读邀请查询：请先完成页面验证，再点击读取。不会发送邀请。 / Complete page verification, then read eligibility. No invitations are sent.';
    const button = document.createElement('button');
    button.textContent = '读取邀请资格 / Read eligibility';
    const status = document.createElement('p');
    button.onclick = async () => {
        button.disabled = true;
        // Desktop referrals are surface-scoped even when using the same OAuth
        // identity. Missing Desktop context can return a valid but hidden offer.
        const headers = { Authorization: 'Bearer ' + CONFIG.token, 'ChatGPT-Account-Id': CONFIG.account_id, Accept: 'application/json', originator: 'Codex Desktop', 'OAI-Product-Sku': 'CODEX' };
        try {
            const usageResponse = await fetch('/backend-api/wham/usage', { method: 'GET', headers, credentials: 'include', redirect: 'error' });
            if (!usageResponse.ok) throw new Error('Identity check HTTP ' + usageResponse.status);
            const usage = await usageResponse.json();
            if (usage.account_id !== CONFIG.account_id || usage.user_id !== CONFIG.user_id) throw new Error('Selected account identity does not match.');
            async function readOffer(entrypoint) {
                const query = new URLSearchParams({ program_id: CONFIG.program, entrypoint });
                const response = await fetch('/backend-api/referrals/invite/eligibility?' + query, { method: 'GET', headers, credentials: 'include', redirect: 'error' });
                if (!response.ok || !response.headers.get('content-type')?.includes('json')) throw new Error(entrypoint + ' eligibility unconfirmed, HTTP ' + response.status + '. Complete page verification and retry.');
                const offer = await response.json();
                if (typeof offer.should_show !== 'boolean') throw new Error('Incomplete eligibility response.');
                return { offer, entrypoint };
            }
            let selected = await readOffer('persistent');
            // The official client also has a separate rate-limit invitation entrypoint.
            // A hidden persistent entrypoint does not establish the result of that query.
            const checkedEntrypoints = ['persistent'];
            if (!selected.offer.should_show) {
                selected = await readOffer('rate_limit');
                checkedEntrypoints.push('rate_limit');
            }
            const result = new URL('referral-result://result');
            result.searchParams.set('nonce', CONFIG.nonce);
            result.searchParams.set('data', JSON.stringify({ account_id: usage.account_id, user_id: usage.user_id, offer: selected.offer, query_entrypoint: selected.entrypoint, checked_entrypoints: checkedEntrypoints }));
            location.href = result.toString();
        } catch (error) {
            status.textContent = error.message;
            button.disabled = false;
        }
    };
    panel.append(note, button, status);
    document.body.appendChild(panel);
}
if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', mount, { once: true });
else mount();
