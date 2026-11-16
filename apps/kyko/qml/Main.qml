import QtQuick
import QtQuick.Window
import org.kyko.app

Window {
    id: root
    readonly property int pad: 8
    visible: true
    color: "#282828"
    function hex(v) { return "#" + ("000000" + v.toString(16)).slice(-6) }
    Controller {
        id: ctl
        // Follows the theme: the background the grids draw on.
        onLookChanged: root.color = root.hex(ctl.setting("background"))
        // The last tab closed: a windowed kyko has nothing left to show.
        onCommand: function (code) { if (code === 3) Qt.quit() }
    }
    title: ctl.fixedTitle().length > 0 ? ctl.fixedTitle()
           : ws.title.length > 0 ? "kyko: " + ws.title : "kyko"

    Workspace {
        id: ws
        ctl: ctl
        anchors.fill: parent
        anchors.margins: root.pad
    }

    // The first size: 100 columns by 28 rows of the first pane, under the tab bar.
    Component.onCompleted: {
        color = hex(ctl.setting("background"))
        const probe = Qt.createQmlObject('import org.kyko.app; GridView {}', ws)
        width = Math.round(probe.cellWidth() * 100 + 2 * pad)
        height = Math.round(probe.cellHeight() * 28 + 2 * pad + ws.barH + 4)
        probe.destroy()
        ws.focusPane()
    }

    // Test aid: KYKO_TEST_SHOT=path saves the window to a PNG after a moment and quits.
    Timer {
        interval: ctl.shotDelay()
        running: ctl.shot().length > 0
        onTriggered: ws.grabToImage(function (r) { r.saveToFile(ctl.shot()); Qt.quit() })
    }
}
