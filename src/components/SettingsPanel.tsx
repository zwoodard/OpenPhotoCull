import { useStore } from "../store";

export function SettingsPanel({ onClose }: { onClose: () => void }) {
  const settings = useStore((s) => s.settings);
  const setSetting = useStore((s) => s.setSetting);
  const analysisMap = useStore((s) => s.analysisMap);

  // Count how many images are flagged at current thresholds
  const entries = Object.values(analysisMap);
  const blurryCount = entries.filter(
    (a) => a.blur && a.blur.laplacianVariance < settings.blurThreshold,
  ).length;
  const exposureCount = entries.filter(
    (a) =>
      a.exposure &&
      (a.exposure.pctUnderexposed > settings.exposureThreshold ||
        a.exposure.pctOverexposed > settings.exposureThreshold),
  ).length;

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        background: "rgba(0,0,0,0.6)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        zIndex: 100,
      }}
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        style={{
          background: "var(--bg-secondary)",
          border: "1px solid var(--border)",
          borderRadius: "var(--radius)",
          padding: 24,
          width: 480,
          maxHeight: "80vh",
          overflow: "auto",
        }}
      >
        <div
          style={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
            marginBottom: 20,
          }}
        >
          <h2 style={{ fontSize: 18, fontWeight: 700 }}>Analysis Settings</h2>
          <button
            onClick={onClose}
            style={{
              background: "var(--bg-surface)",
              color: "var(--text-primary)",
              padding: "4px 12px",
              borderRadius: "var(--radius-sm)",
              fontSize: 13,
            }}
          >
            Done
          </button>
        </div>

        {/* Blur threshold */}
        <SettingSlider
          label="Blur Detection"
          description="How sharp a photo must be to pass. Higher values flag more photos as blurry."
          value={settings.blurThreshold}
          min={20}
          max={500}
          step={10}
          onChange={(v) => setSetting("blurThreshold", v)}
          displayValue={`${settings.blurThreshold}`}
          lowLabel="Lenient"
          highLabel="Aggressive"
          badge={`${blurryCount} flagged`}
          badgeColor={blurryCount > 0 ? "var(--warning)" : "var(--success)"}
        />

        {/* Exposure threshold */}
        <SettingSlider
          label="Exposure Detection"
          description="What percentage of pixels must be in extreme dark/bright zones to flag. Lower values flag more photos."
          value={settings.exposureThreshold}
          min={0.05}
          max={0.6}
          step={0.05}
          onChange={(v) => setSetting("exposureThreshold", v)}
          displayValue={`${Math.round(settings.exposureThreshold * 100)}%`}
          lowLabel="Aggressive"
          highLabel="Lenient"
          badge={`${exposureCount} flagged`}
          badgeColor={exposureCount > 0 ? "var(--warning)" : "var(--success)"}
        />

        {/* Duplicate threshold */}
        <SettingSlider
          label="Duplicate Sensitivity"
          description="How similar photos must be to group as duplicates. Higher values match more loosely."
          value={settings.duplicateThreshold}
          min={2}
          max={20}
          step={1}
          onChange={(v) => setSetting("duplicateThreshold", v)}
          displayValue={`${settings.duplicateThreshold}`}
          lowLabel="Strict"
          highLabel="Loose"
        />

        {/* Scene window */}
        <SettingSlider
          label="Scene Grouping Window"
          description="How many seconds between photos before starting a new scene. Photos taken within this window are grouped together."
          value={settings.sceneWindowSecs}
          min={10}
          max={120}
          step={5}
          onChange={(v) => setSetting("sceneWindowSecs", v)}
          displayValue={`${settings.sceneWindowSecs}s`}
          lowLabel="Tight (10s)"
          highLabel="Loose (120s)"
        />

        <div
          style={{
            marginTop: 16,
            padding: 12,
            background: "var(--bg-primary)",
            borderRadius: "var(--radius-sm)",
            fontSize: 12,
            color: "var(--text-muted)",
            lineHeight: 1.5,
          }}
        >
          Blur and exposure thresholds apply instantly. Duplicate sensitivity
          and scene window require a re-scan to take effect.
        </div>
      </div>
    </div>
  );
}

function SettingSlider({
  label,
  description,
  value,
  min,
  max,
  step,
  onChange,
  displayValue,
  lowLabel,
  highLabel,
  badge,
  badgeColor,
}: {
  label: string;
  description: string;
  value: number;
  min: number;
  max: number;
  step: number;
  onChange: (v: number) => void;
  displayValue: string;
  lowLabel: string;
  highLabel: string;
  badge?: string;
  badgeColor?: string;
}) {
  return (
    <div style={{ marginBottom: 20 }}>
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
          marginBottom: 4,
        }}
      >
        <span style={{ fontSize: 14, fontWeight: 600 }}>{label}</span>
        <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
          {badge && (
            <span
              style={{
                fontSize: 11,
                fontWeight: 700,
                padding: "2px 6px",
                borderRadius: "var(--radius-sm)",
                background: badgeColor || "var(--text-muted)",
                color: "#000",
              }}
            >
              {badge}
            </span>
          )}
          <span
            style={{
              fontSize: 13,
              fontWeight: 700,
              color: "var(--accent)",
              minWidth: 40,
              textAlign: "right",
            }}
          >
            {displayValue}
          </span>
        </div>
      </div>
      <p
        style={{
          fontSize: 11,
          color: "var(--text-muted)",
          marginBottom: 8,
          lineHeight: 1.4,
        }}
      >
        {description}
      </p>
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        onChange={(e) => onChange(parseFloat(e.target.value))}
        style={{ width: "100%", accentColor: "var(--accent)" }}
      />
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          fontSize: 10,
          color: "var(--text-muted)",
          marginTop: 2,
        }}
      >
        <span>{lowLabel}</span>
        <span>{highLabel}</span>
      </div>
    </div>
  );
}
