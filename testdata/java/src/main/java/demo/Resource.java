package demo;
import jakarta.ws.rs.POST;
import jakarta.ws.rs.Path;
import jakarta.transaction.Transactional;
import org.eclipse.microprofile.reactive.messaging.Emitter;
@Path("/journals")
public class Resource {
  Emitter<String> emitter;
  @POST
  @Transactional
  public void create(String journal) {
    emitter.send(journal);
  }
}
