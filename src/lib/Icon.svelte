<script lang="ts">
  import { ICONS, type IconName } from "./icons";

  interface Props {
    name: IconName;
    size?: number;
    /** Accessible name. Leave out for decorative icons next to a text label. */
    label?: string;
  }

  let { name, size = 16, label }: Props = $props();
</script>

<svg class="icon" width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor"
  stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"
  role={label ? "img" : undefined} aria-label={label} aria-hidden={label ? undefined : "true"}>
  {#each ICONS[name] as d (d)}<path {d} />{/each}
</svg>

<style>
  .icon {
    display: inline-block;
    flex-shrink: 0;
    vertical-align: -0.15em;
  }
</style>
