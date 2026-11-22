// kyko's page in uQ Settings: the drop-down terminal's position, size, dim and behaviour. Reads
// and writes go through `kyko config` (page.toml); the running drop-down follows its config file.
import QtQuick
import QtQuick.Layouts
import org.uq.settings
import "logic.js" as Logic

SettingsPage {
    id: page

    property var ctx
    property string highlight
    property var values: null
    property string problem: ""
    readonly property string position: values ? values["window.position"] : "top"
    readonly property var sizeKeys: Logic.sizeKeys(position)

    pageId: "kyko"

    function show(result) {
        const v = Logic.valuesOf(result);
        if (v !== null)
            page.values = v;
        page.problem = Logic.problemOf(result);
    }

    function set(key, value) {
        page.ctx.command.run("set", [key, String(value)], function (result) {
            page.problem = Logic.problemOf(result);
            page.ctx.command.run("config", [], page.show);
        });
    }

    function refresh() {
        if (page.ctx)
            page.ctx.command.run("config", [], page.show);
    }

    onCtxChanged: {
        if (page.ctx)
            page.ctx.command.watch("config", page.show);
    }

    banners: [
        ErrorCard {
            visible: page.problem !== ""
            title: "kyko's settings"
            detail: page.problem
        }
    ]

    SettingsTab {
        title: "Drop-down"
        iconName: "org.kyko.Kyko"

        SettingsCard {
            title: "Where it shows"
            iconName: "view-split-top-bottom"
            help: "F12 shows and hides it; change the key in Shortcuts."

            SettingRow {
                settingId: "position"
                label: "Comes from"
                highlight: page.highlight
                UqComboBox {
                    model: Logic.POSITION_LABELS
                    enabled: page.values !== null
                    currentIndex: Math.max(0, Logic.POSITIONS.indexOf(page.position))
                    onActivated: function (i) {
                        page.set("window.position", Logic.POSITIONS[i]);
                    }
                }
            }
            SettingRow {
                settingId: "shape"
                label: "Shape"
                highlight: page.highlight
                UqComboBox {
                    model: ["Island, with a gap", "Sheet, edge to edge"]
                    enabled: page.values !== null
                    currentIndex: page.values && page.values["window.shape"] === "sheet" ? 1 : 0
                    onActivated: function (i) {
                        page.set("window.shape", i === 1 ? "sheet" : "island");
                    }
                }
            }
            ValueSlider {
                settingId: "size"
                label: "Width"
                from: 20
                to: 100
                stepSize: 1
                lowLabel: "20%"
                highLabel: "100%"
                enabled: page.values !== null
                value: page.values ? page.values[page.sizeKeys[0]] : 88
                onChosen: function (v) {
                    page.set(page.sizeKeys[0], Math.round(v));
                }
            }
            ValueSlider {
                label: "Height"
                from: 20
                to: 100
                stepSize: 1
                lowLabel: "20%"
                highLabel: "100%"
                enabled: page.values !== null
                value: page.values ? page.values[page.sizeKeys[1]] : 45
                onChosen: function (v) {
                    page.set(page.sizeKeys[1], Math.round(v));
                }
            }
            UqLabel {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                opacity: 0.7
                text: "Each position keeps its own size. You can also drag the drop-down's edge."
            }
        }

        SettingsCard {
            title: "Behaviour"
            iconName: "preferences-desktop"

            ValueSlider {
                settingId: "dim"
                label: "Dim the screen behind"
                from: 0
                to: 80
                stepSize: 5
                lowLabel: "Off"
                highLabel: "80%"
                enabled: page.values !== null
                value: page.values ? page.values["window.dim_pct"] : 40
                onChosen: function (v) {
                    page.set("window.dim_pct", Math.round(v));
                }
            }
            SwitchRow {
                settingId: "behaviour"
                label: "Hide when it loses focus"
                highlight: page.highlight
                enabled: page.values !== null
                value: page.values ? page.values["window.hide_on_focus_loss"] === true : true
                onChosen: function (v) {
                    page.set("window.hide_on_focus_loss", v);
                }
            }
            SwitchRow {
                label: "Show over full-screen windows"
                enabled: page.values !== null
                value: page.values ? page.values["window.above_fullscreen"] === true : true
                onChosen: function (v) {
                    page.set("window.above_fullscreen", v);
                }
            }
            SwitchRow {
                label: "Slide in and out"
                enabled: page.values !== null
                value: page.values ? page.values["window.animations"] === true : true
                onChosen: function (v) {
                    page.set("window.animations", v);
                }
            }
        }
    }
}
