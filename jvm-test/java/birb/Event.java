package birb;

public class Event {
    protected String message;

    public Event() {

    }

    public Event(String message) {
        this.message = message;
    }

    public String getMessage() {
        return this.message;
    }
}