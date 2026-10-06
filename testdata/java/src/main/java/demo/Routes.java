package demo;
import org.apache.camel.builder.RouteBuilder;
public class Routes extends RouteBuilder {
  public void configure() {
    from("direct:input").routeId("input")
      .choice().when(simple("${body} != null"))
        .to("direct:process")
      .otherwise().to("jpa:Discarded").end();
    from("direct:process").routeId("process")
      .split(body()).doTry().to("https://example.com/ocr")
      .doCatch(Exception.class).to("jpa:Failure").end()
      .wireTap("google-pubsub:demo:results").end();
    from("direct:events").routeId("events")
      .transacted().to("direct:input");
  }
}
