import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import {
    formatReferralReward,
    hasKnownReferralReward,
    referralProgramLabel,
    referralRewardCapacity,
    referralSendCapacity,
    type ReferralInvite,
    type ReferralOffer,
    type ReferralProgram,
    type ReferralSendResult,
    type ReferralTracking,
} from './referral';
import './ReferralInviteModal.css';

export function ReferralInviteModal({ id, name, program: initialProgram, onClose }: { id: string; name: string; program: ReferralProgram; onClose: () => void }) {
    const [program, setProgram] = useState<ReferralProgram>(initialProgram);
    const [offer, setOffer] = useState<ReferralOffer | null>(null);
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState('');
    const [input, setInput] = useState('');
    const [confirmed, setConfirmed] = useState(false);
    const [sending, setSending] = useState(false);
    const [result, setResult] = useState<ReferralSendResult | null>(null);
    const [records, setRecords] = useState<ReferralInvite[]>([]);
    const [cursor, setCursor] = useState<string | null>(null);
    const [trackingLoading, setTrackingLoading] = useState(false);
    const [trackingLoaded, setTrackingLoaded] = useState(false);
    const [trackingError, setTrackingError] = useState('');
    const [submitted, setSubmitted] = useState(false);
    const generation = useRef(0);
    const sendLock = useRef(false);

    async function refreshOffer() {
        const gen = ++generation.current;
        setLoading(true); setError(''); setOffer(null); setConfirmed(false);
        try {
            const data = await invoke<ReferralOffer>('get_desktop_referral_eligibility', { id, program });
            if (gen === generation.current) setOffer(data);
        } catch (e) { if (gen === generation.current) setError(String(e)); }
        finally { if (gen === generation.current) setLoading(false); }
    }
    useEffect(() => {
        setInput(''); setResult(null); setRecords([]); setCursor(null);
        setTrackingLoaded(false); setTrackingError(''); setSubmitted(false);
        void refreshOffer();
        return () => { generation.current++; };
    }, [id, program]);

    async function tracking(more = false) {
        const gen = generation.current;
        setTrackingLoading(true); setTrackingError('');
        try {
            const data = await invoke<ReferralTracking>('get_desktop_referral_tracking', { id, program, cursor: more ? cursor : null });
            if (gen !== generation.current) return;
            setRecords(prev => more ? [...prev, ...data.items] : data.items);
            setCursor(data.cursor ?? null); setTrackingLoaded(true);
        } catch (e) { if (gen === generation.current) setTrackingError(String(e)); }
        finally { setTrackingLoading(false); }
    }
    const emails = [...new Map(input.split(/[\s,;]+/).filter(Boolean).map(e => [e.toLowerCase(), e])).values()];
    const cap = referralSendCapacity(offer);
    const rewardCapacity = referralRewardCapacity(offer);
    const knownReward = hasKnownReferralReward(offer);
    const valid = emails.length > 0 && emails.length <= cap && emails.every(e => /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(e));
    const ready = valid && !!offer && (offer.requires_explicit_confirmation === false || confirmed);
    async function send() {
        if (!ready || sendLock.current || submitted) return;
        sendLock.current = true; setSending(true); setSubmitted(true); setError('');
        try {
            setResult(await invoke<ReferralSendResult>('send_desktop_referral_invite', { id, program, emails, expected: offer }));
            await tracking();
        } catch (e) { setError(String(e)); }
        finally { setSending(false); sendLock.current = false; }
    }
    const workspaceInvite = program === 'codex_referral_workspace';
    return <div className="modal-overlay referral-overlay" onClick={() => !sending && onClose()}>
        <div
            className="modal-content referral-modal"
            role="dialog"
            aria-modal="true"
            aria-labelledby="referral-modal-title"
            onClick={e => e.stopPropagation()}
        >
            <div className="modal-header referral-modal-header">
                <div className="referral-title-block">
                    <span className="referral-eyebrow">ACCOUNT INVITE</span>
                    <h2 id="referral-modal-title">{workspaceInvite ? '邀请同事使用 ChatGPT 桌面版' : '邀请朋友使用 ChatGPT 桌面版'}</h2>
                    <p>{workspaceInvite ? '邀请同事加入工作区，奖励按官方活动规则发放' : '邀请朋友加入，奖励按官方活动规则发放'}</p>
                </div>
                <button className="close-btn referral-close" onClick={onClose} disabled={sending} aria-label="关闭邀请窗口">×</button>
            </div>

            <div className="modal-body referral-modal-body">
                <div className="referral-account-strip">
                    <span className="referral-account-label">发邀账号</span>
                    <strong>{name}</strong>
                    <span className="referral-account-tag">{referralProgramLabel(program)}</span>
                </div>

                <section className="referral-section referral-offer-section">
                    <div className="referral-section-heading">
                        <div>
                            <span className="referral-eyebrow">OFFER</span>
                            <h3>当前活动</h3>
                        </div>
                        <div className="referral-program-control">
                            <span>活动范围</span>
                            <select value={program} onChange={e => setProgram(e.target.value as ReferralProgram)} disabled={sending || loading || trackingLoading}>
                                <option value="codex_referral_consumer">个人账号</option>
                                <option value="codex_referral_workspace">工作区</option>
                            </select>
                            <button className="referral-text-button" onClick={() => void refreshOffer()} disabled={loading || sending || trackingLoading}>
                                {loading ? '查询中…' : '刷新资格'}
                            </button>
                        </div>
                    </div>

                    {loading && <p className="referral-muted" role="status">正在查询活动资格…</p>}
                    {offer && <>
                        <div className={`referral-offer-summary${offer.should_show ? '' : ' unavailable'}`}>
                            <div className="referral-reward-block">
                                <span className="referral-stat-label">每位符合条件的邀请奖励</span>
                                <strong>{offer.should_show ? formatReferralReward(offer) : '当前账号暂未开放'}</strong>
                            </div>
                            {offer.should_show && <div className="referral-capacity-block">
                                <span className="referral-stat-label">可发送邮箱</span>
                                <strong>{cap}</strong>
                                <span>人</span>
                            </div>}
                        </div>
                        {offer.should_show && <div className="referral-offer-meta">
                            <span>发送上限 {offer.remaining_send_capacity ?? '未提供'}</span>
                            <span>奖励名额 {offer.remaining_reward_capacity ?? '未提供'}</span>
                        </div>}
                        <p className="referral-note">
                            {!knownReward
                                ? '接口未提供实际奖励金额，不能根据活动编号推断为 500 或 1000。'
                                : '活动奖励不等于当前余额；对方接受邀请并完成官方要求后才会到账。'}
                            {rewardCapacity === 0 && ' 当前奖励名额为 0，已禁止发送邀请。'}
                        </p>
                    </>}
                </section>

                <section className="referral-section referral-compose-section">
                    <div className="referral-section-heading">
                        <div>
                            <span className="referral-eyebrow">RECIPIENTS</span>
                            <h3>填写受邀邮箱</h3>
                        </div>
                        {offer?.should_show && <span className="referral-section-hint">
                            {rewardCapacity === 0 ? '奖励名额已用完' : `最多发送 ${cap} 个`}
                        </span>}
                    </div>
                    <textarea
                        className="referral-email-input"
                        aria-label="受邀邮箱"
                        value={input}
                        onChange={e => setInput(e.target.value)}
                        rows={3}
                        placeholder="每行一个邮箱，也可用逗号或分号分隔"
                        disabled={sending || submitted}
                    />
                    {input && !valid && <p className="referral-validation" role="alert">请检查邮箱格式和本次可发送人数（最多 {cap} 个）。</p>}
                    {offer && offer.requires_explicit_confirmation !== false && <label className="referral-confirm">
                        <input type="checkbox" checked={confirmed} onChange={e => setConfirmed(e.target.checked)} disabled={sending || submitted} />
                        <span>我已确认收件人及奖励条件，发送邀请不代表奖励已到账。</span>
                    </label>}
                </section>

                {error && <div className="referral-result-card error" role="alert">{error}</div>}
                {result && <section className="referral-result-section" role="status">
                    <div className="referral-section-heading">
                        <div>
                            <span className="referral-eyebrow">RESULT</span>
                            <h3>发送结果</h3>
                        </div>
                    </div>
                    {result.invites.length > 0 && <p className="referral-result-summary">上游已创建 {result.invites.length} 条邀请</p>}
                    {result.grants && <p className="referral-result-summary">本次奖励：{formatReferralReward(result)}</p>}
                    {result.invites.map((v, i) => <div className="referral-result-card success" key={v.referral_id || i}>{v.email || '邀请记录已创建'}</div>)}
                    {!!result.failed_emails?.length && <div className="referral-result-card error">未成功：{result.failed_emails.join('、')} {result.message}</div>}
                    {!result.invites.length && !result.failed_emails?.length && <p className="referral-muted">未返回已创建的邀请，请查询记录确认。</p>}
                </section>}

                {submitted && <p className="referral-submitted">本次提交已结束。需要再次邀请时，请先核对记录，再重新打开此窗口。</p>}

                <section className="referral-history-section">
                    <div className="referral-history-heading">
                        <div>
                            <span className="referral-eyebrow">HISTORY</span>
                            <h3>邀请记录</h3>
                        </div>
                        <button className="referral-text-button" onClick={() => void tracking()} disabled={trackingLoading || sending || loading}>
                            {trackingLoading ? '查询中…' : '查询近 90 天'}
                        </button>
                    </div>
                    {trackingError && <p className="referral-validation" role="alert">{trackingError}</p>}
                    {trackingLoaded && !records.length && <p className="referral-muted">近 90 天没有邀请记录。</p>}
                    {records.map((v, i) => <div className="referral-history-row" key={`${v.referral_id}-${i}`}>
                        <span>{v.email || '—'}</span>
                        <span>{v.status || '上游未提供状态'}</span>
                    </div>)}
                    {cursor && <button className="referral-load-more" onClick={() => void tracking(true)} disabled={trackingLoading || sending || loading}>加载更多</button>}
                </section>
            </div>

            <div className="modal-footer referral-modal-footer">
                <span className="referral-footer-hint">奖励到账以官方记录为准</span>
                <div className="referral-footer-actions">
                    <button className="btn btn-ghost" onClick={onClose} disabled={sending}>关闭</button>
                    <button className="btn btn-primary referral-send-button" onClick={() => void send()} disabled={!ready || sending || loading || submitted}>
                        {sending ? '发送中…' : '发送邀请'}
                    </button>
                </div>
            </div>
        </div>
    </div>;
}
