<script lang="ts">
    interface Props {
        title: string;
        type?: string;
    }
    let { title }: Props = $props();
</script>

<div class="panel-header">
    <span class="title">{title}</span>
    <!-- İleride buraya özel ikonlar veya menüler eklenebilir -->
</div>

<style>
    .panel-header {
        height: 32px; /* Standart panel başlık yüksekliği */
        display: flex;
        align-items: center;
        padding: 0 12px;
        flex-shrink: 0;
        user-select: none;
        /* Arka plan rengi Panel bileşeninden gelebilir veya şeffaf olabilir */
        width: 100%;
    }

    .title {
        font-size: 11px;
        font-weight: 600;
        text-transform: uppercase;
        color: #999;
        letter-spacing: 0.5px;
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }
</style>
