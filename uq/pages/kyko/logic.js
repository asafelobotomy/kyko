.pragma library
// Pure logic for kyko's uQ Settings page: no Qt calls, so Node can test it.

var POSITIONS = ["top", "bottom", "left", "right", "center"];
var POSITION_LABELS = ["Top, below the bar", "Bottom", "Left side", "Right side", "Middle of the screen"];

// The config keys that hold the size where the drop-down is now (kyko's Position::size_keys).
function sizeKeys(position) {
    if (position === "left" || position === "right")
        return ["window.side_width_pct", "window.side_height_pct"];
    if (position === "center")
        return ["window.center_width_pct", "window.center_height_pct"];
    return ["window.width_pct", "window.height_pct"];
}

// The settings from `kyko config show --json`, or null when the answer is not usable.
function valuesOf(result) {
    if (!result || result.code !== 0 || !result.output || typeof result.output.values !== "object")
        return null;
    return result.output.values;
}

// What went wrong, as one line; "" when nothing did.
function problemOf(result) {
    if (!result)
        return "kyko did not answer.";
    if (result.output && result.output.ok === false)
        return result.output.error || "kyko refused the change.";
    if (result.output && typeof result.output.error === "string" && result.output.error !== "")
        return result.output.error;
    if (result.code !== 0 || result.error)
        return result.error || "kyko failed.";
    return "";
}
