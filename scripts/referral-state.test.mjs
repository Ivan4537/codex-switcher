import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';
const loaded = { exports: {} };
vm.runInNewContext(ts.transpileModule(fs.readFileSync('src/components/referral.ts', 'utf8'), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
}).outputText, { module: loaded, exports: loaded.exports });
const { parseReferralOffer, referralSendCapacity } = loaded.exports;

test('no activity requires an explicit upstream false', () => {
    assert.throws(() => parseReferralOffer({}), /未确认/);
    assert.throws(() => parseReferralOffer({ should_show: true }), /未确认/);
    assert.throws(() => parseReferralOffer({ should_show: true, remaining_send_capacity: 3, grants: {} }), /未确认/);
    assert.throws(() => parseReferralOffer({ should_show: 'false', remaining_send_capacity: 3 }), /未确认/);
    assert.equal(parseReferralOffer({ should_show: false }).should_show, false);
});

test('rewardless campaign retains three real sending slots', () => {
    const offer = parseReferralOffer({ should_show: true, remaining_send_capacity: 3,
        remaining_reward_capacity: 0, grants: [], offer_id: 'none' });
    assert.equal(referralSendCapacity(offer), 3);
});

test('reward campaigns and legacy offers still honor reward limits', () => {
    assert.equal(referralSendCapacity({ should_show: true, remaining_send_capacity: 5,
        remaining_reward_capacity: 0, grants: [{ amount: 1000 }] }), 0);
    assert.equal(referralSendCapacity({ should_show: true, remaining_send_capacity: 5,
        remaining_reward_capacity: 2, grants: [], offer_id: 'credits_1000' }), 2);
});
