import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import QtQuick.Window
import org.kyko.app

// kyko's Preferences (spec 2.6): an ordinary window over config.toml. Every change is written at
// once; the drop-down re-reads the file, so there is no Apply.
Window {
    id: root
    visible: true
    width: 760
    height: 520
    minimumWidth: 560
    minimumHeight: 380
    title: "kyko Preferences"

    function hex(v) { return "#" + ("000000" + v.toString(16)).slice(-6) }
    property color bg: hex(ctl.setting("background"))
    property color fg: hex(ctl.setting("foreground"))
    property color accent: hex(ctl.setting("accent"))
    readonly property color dim: Qt.rgba(fg.r, fg.g, fg.b, 0.6)
    readonly property color panel: Qt.rgba(fg.r, fg.g, fg.b, 0.08)
    color: bg
    palette.window: bg
    palette.windowText: fg
    palette.base: Qt.rgba(fg.r, fg.g, fg.b, 0.12)
    palette.text: fg
    palette.button: Qt.rgba(fg.r, fg.g, fg.b, 0.14)
    palette.buttonText: fg
    palette.highlight: accent
    palette.highlightedText: bg
    palette.mid: Qt.rgba(fg.r, fg.g, fg.b, 0.35)
    palette.dark: Qt.rgba(fg.r, fg.g, fg.b, 0.5)

    property var pages: JSON.parse(ctl.prefsPages())
    property int page: 0
    property var vals: ({})
    property string fileError: ""
    property string status: ""

    Controller { id: ctl }

    function reload() {
        const v = JSON.parse(ctl.prefsValues())
        vals = v.values
        fileError = v.error
    }
    // Writes one setting; shows why when it is refused and puts the shown value back.
    function save(key, value) {
        const why = ctl.prefsSet(key, JSON.stringify(value))
        status = why
        reload()
    }
    Component.onCompleted: reload()

    RowLayout {
        anchors.fill: parent
        spacing: 0

        // ---- the pages ------------------------------------------------------------------
        Rectangle {
            Layout.fillHeight: true
            Layout.preferredWidth: 170
            color: root.panel
            ListView {
                id: sidebar
                anchors.fill: parent
                anchors.margins: 8
                spacing: 2
                model: root.pages
                delegate: Rectangle {
                    required property var modelData
                    required property int index
                    width: sidebar.width
                    height: 34
                    radius: 6
                    color: root.page === index ? root.accent : "transparent"
                    Text {
                        anchors.verticalCenter: parent.verticalCenter
                        x: 12
                        text: modelData.title
                        color: root.page === index ? root.bg : root.fg
                        font.pixelSize: 14
                    }
                    MouseArea { anchors.fill: parent; onClicked: root.page = index }
                }
            }
        }

        // ---- one page -------------------------------------------------------------------
        ColumnLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.margins: 20
            spacing: 10

            Text {
                visible: root.fileError.length > 0
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                color: "#fb4934"
                text: root.fileError + ". Fix the file by hand; changes here are refused until it parses."
            }

            Flickable {
                Layout.fillWidth: true
                Layout.fillHeight: true
                contentHeight: items.implicitHeight
                clip: true
                ColumnLayout {
                    id: items
                    width: parent.width - 12
                    spacing: 16
                    Repeater {
                        model: root.pages[root.page].items
                        delegate: RowLayout {
                            id: row
                            required property var modelData
                            readonly property string key: modelData.key
                            readonly property var current: root.vals[key]
                            Layout.fillWidth: true
                            // A fixed row height keeps the rows where the test clicks, whatever the font.
                            Layout.preferredHeight: 40
                            spacing: 16
                            ColumnLayout {
                                Layout.fillWidth: true
                                spacing: 2
                                Text {
                                    Layout.fillWidth: true
                                    text: row.modelData.label
                                    color: root.fg
                                    font.pixelSize: 14
                                }
                                Text {
                                    visible: row.modelData.help.length > 0
                                    Layout.fillWidth: true
                                    text: row.modelData.help
                                    color: root.dim
                                    wrapMode: Text.Wrap
                                    font.pixelSize: 12
                                }
                            }
                            Loader {
                                Layout.alignment: Qt.AlignRight | Qt.AlignVCenter
                                sourceComponent: {
                                    switch (row.modelData.type) {
                                    case "bool": return boolControl
                                    case "int": case "optint": return intControl
                                    case "choice": return choiceControl
                                    case "key": return keyControl
                                    default: return textControl
                                    }
                                }
                                property alias item_: row
                            }
                        }
                    }
                }
            }

            RowLayout {
                Layout.fillWidth: true
                Text {
                    Layout.fillWidth: true
                    text: root.status
                    color: "#fb4934"
                    wrapMode: Text.Wrap
                    elide: Text.ElideRight
                }
                Button {
                    text: "Reset this page"
                    onClicked: {
                        root.status = ctl.prefsReset(root.pages[root.page].id)
                        root.reload()
                    }
                }
            }
        }
    }

    // ---- the controls: each reads `item_.current` and saves through `root.save` ------------
    Component {
        id: boolControl
        Switch {
            checked: item_.current === true
            onToggled: root.save(item_.key, checked)
        }
    }
    Component {
        id: intControl
        SpinBox {
            readonly property bool optional: item_.modelData.type === "optint"
            editable: true
            from: optional ? 0 : item_.modelData.min
            to: item_.modelData.max
            value: item_.current
            // An optional number is 0 (follow the theme) or in range: step over the gap.
            textFromValue: function (v) { return optional && v === 0 ? "theme" : v }
            valueFromText: function (t) { return optional && t === "theme" ? 0 : Number(t) }
            onValueModified: {
                if (optional && value > 0 && value < item_.modelData.min)
                    value = item_.current === 0 ? item_.modelData.min : 0
                root.save(item_.key, value)
            }
        }
    }
    Component {
        id: choiceControl
        ComboBox {
            model: item_.modelData.options
            currentIndex: model.indexOf(item_.current)
            onActivated: root.save(item_.key, model[currentIndex])
        }
    }
    Component {
        id: textControl
        TextField {
            implicitWidth: 200
            text: item_.current
            // Empty follows the theme: show the font that is in use.
            placeholderText: root.vals["terminal.font_now"] || ""
            onEditingFinished: root.save(item_.key, text)
        }
    }
    // A key: click, then press the sequence. Esc cancels, Backspace clears.
    Component {
        id: keyControl
        Button {
            id: capture
            property bool listening: false
            implicitWidth: 200
            text: listening ? "Press a key…" : (item_.current === "" ? "none" : item_.current)
            onClicked: { listening = true; forceActiveFocus() }
            onActiveFocusChanged: if (!activeFocus) listening = false
            Keys.onPressed: function (e) {
                if (!listening) return
                e.accepted = true
                if (e.modifiers === Qt.NoModifier && e.key === Qt.Key_Escape) {
                    listening = false
                    return
                }
                const seq = e.modifiers === Qt.NoModifier && e.key === Qt.Key_Backspace
                    ? "" : ctl.prefsKey(e.key, e.modifiers, e.text)
                // Only a modifier so far: keep waiting.
                if (seq === "" && e.key !== Qt.Key_Backspace) return
                listening = false
                root.save(item_.key, seq)
            }
        }
    }
}
