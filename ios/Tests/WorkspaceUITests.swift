import XCTest

final class WorkspaceUITests: XCTestCase {
    func testNativeWorkspaceScreens() {
        let app = XCUIApplication()
        app.launch()
        XCTAssertTrue(app.textFields["promptField"].waitForExistence(timeout: 15))
        attach("Conversation", app: app)
        for page in ["Files", "Activity", "Settings"] {
            let tab = app.tabBars.buttons[page]
            if tab.exists { tab.tap() } else { app.buttons[page].firstMatch.tap() }
            XCTAssertTrue(app.navigationBars[page].waitForExistence(timeout: 5))
            attach(page, app: app)
        }
    }

    private func attach(_ name: String, app: XCUIApplication) {
        let attachment = XCTAttachment(screenshot: app.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
