<script setup>
// One progress meter for the generated status page (docs/status/index.md).
// It only draws what tools/docs/render_status.py hands it: `value` and
// `total` are the source's own counts, and the percentage is computed here
// from them, never passed in separately. `stale` greys the bar out and
// labels it, for a figure that is no longer current.
import { computed } from 'vue'

const props = defineProps({
  label: { type: String, required: true },
  value: { type: Number, required: true },
  total: { type: Number, required: true },
  note: { type: String, default: '' },
  stale: { type: Boolean, default: false }
})

const share = computed(() => (props.total > 0 ? (100 * props.value) / props.total : 0))
const percent = computed(() => `${share.value.toFixed(2)}%`)
const fmt = (n) => (Number.isInteger(n) ? n.toLocaleString('en-US') : String(n))
const counts = computed(() =>
  props.total === 100 && !Number.isInteger(props.value) ? '' : `${fmt(props.value)} / ${fmt(props.total)}`
)
</script>

<template>
  <div class="status-meter" :class="{ 'is-stale': stale }">
    <div class="status-meter__head">
      <span class="status-meter__label">{{ label }}</span>
      <span class="status-meter__value">
        <span v-if="stale" class="status-meter__badge">STALE</span>
        {{ percent }}
      </span>
    </div>
    <div
      class="status-meter__track"
      role="progressbar"
      :aria-label="label"
      :aria-valuenow="Number(share.toFixed(2))"
      aria-valuemin="0"
      aria-valuemax="100"
    >
      <div class="status-meter__fill" :style="{ width: `${Math.min(share, 100)}%` }" />
    </div>
    <div class="status-meter__foot">
      <span v-if="counts" class="status-meter__counts">{{ counts }}</span>
      <span class="status-meter__note">{{ note }}</span>
    </div>
  </div>
</template>

<style scoped>
.status-meter {
  padding: 12px 14px;
  border: 1px solid var(--vp-c-divider);
  border-radius: 8px;
  background: var(--vp-c-bg-soft);
  min-width: 0;
}
.status-meter__head {
  display: flex;
  justify-content: space-between;
  align-items: baseline;
  gap: 8px;
}
.status-meter__label {
  font-size: 14px;
  font-weight: 600;
  line-height: 1.35;
  color: var(--vp-c-text-1);
}
.status-meter__value {
  font-size: 18px;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
  color: var(--vp-c-brand-1);
  white-space: nowrap;
}
.status-meter__track {
  margin: 8px 0 6px;
  height: 8px;
  border-radius: 4px;
  background: var(--vp-c-default-soft);
  overflow: hidden;
}
.status-meter__fill {
  height: 100%;
  min-width: 2px;
  border-radius: 4px;
  background: var(--vp-c-brand-1);
}
.status-meter__foot {
  display: flex;
  flex-wrap: wrap;
  gap: 2px 10px;
  font-size: 12px;
  line-height: 1.4;
  color: var(--vp-c-text-2);
}
.status-meter__counts {
  font-variant-numeric: tabular-nums;
  font-weight: 600;
}
.status-meter__badge {
  display: inline-block;
  margin-right: 6px;
  padding: 0 6px;
  border-radius: 4px;
  font-size: 11px;
  font-weight: 700;
  letter-spacing: 0.04em;
  vertical-align: 2px;
  color: var(--vp-c-danger-1);
  background: var(--vp-c-danger-soft);
}
.is-stale .status-meter__value {
  color: var(--vp-c-text-2);
}
.is-stale .status-meter__fill {
  background: repeating-linear-gradient(
    -45deg,
    var(--vp-c-text-3),
    var(--vp-c-text-3) 4px,
    transparent 4px,
    transparent 8px
  );
}
</style>

<style>
/* The grid that holds the meters; unscoped because the page, not the
   component, renders the wrapper div. One column on a phone. */
.status-meters {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
  gap: 12px;
  margin: 16px 0;
}
</style>
